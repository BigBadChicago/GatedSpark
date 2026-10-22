//! The published package: the bytes a player's device actually receives.
//!
//! `design/content-delivery.md` section 6 asks that every accepted artifact be
//! persisted with a content id, a machine-readable validation report, and a
//! signature and package hash. [`report`](crate::report) is the evidence;
//! this module is the *file*: one canonical byte layout, bound to its own
//! fingerprint, bound in turn to a signature.
//!
//! # The one rule this file exists to enforce
//!
//! **Nothing is parsed out of a package until the bytes it came from have been
//! vouched for.** [`open`] computes the digest of the body, compares it with the
//! content id the file states, asks a [`SignatureVerifier`] about that content id,
//! and only then decodes anything. A caller cannot skip those steps and read the
//! manifest anyway: [`Opened`] has a private field, so this module is the only
//! place one can be made, and [`open`] is the only function that makes one.
//!
//! That order is the whole point. Decoding first and checking afterwards is the
//! usual shape of this bug, and it means every parser in the file is reachable by
//! anyone who can hand the device a byte string.
//!
//! # What this module deliberately does not do
//!
//! It does not decide whether a puzzle may be published. That judgement is
//! [`crate::verification::decide`] and there is exactly one of it. A manifest read
//! back out of a file carries the recorded queries so an audit can read them, and
//! this module offers no function that looks at them and says yes. See the note on
//! [`Manifest::queries`] for the one piece of friction that follows from it, which
//! is open rather than hidden.
//!
//! # Unenforced here
//!
//! No cryptography ships in this crate. [`Digest`] has no implementation (see
//! `docs/adr/0004-dependencies.md`) and neither [`Signer`] nor
//! [`SignatureVerifier`] does, which is what stops this crate from signing
//! anything with a key of its own invention or verifying its own signatures. The
//! test implementations in `tests/` exercise the *binding* discipline and are named
//! so that using one to publish would be obvious.

use crate::content::{ContentId, Digest};
use crate::hash::StateHash;
use crate::report::{Measurement, Replay, ValidationReport};
use crate::verification::{Outcome, Query};

/// The first bytes of every package file.
pub const MAGIC: [u8; 8] = *b"GATEDSPK";

/// The container version.
///
/// Bumped when the *layout* changes. A reader that does not know a version refuses
/// the file rather than guessing at it, which is [`OpenError::UnsupportedFormat`].
/// This is not the ruleset version, which lives in the manifest and says whether
/// the puzzle's semantics are comparable.
pub const FORMAT_VERSION: u16 = 1;

/// The length of a content id in the file, in bytes.
const CONTENT_ID_LEN: usize = 32;

// Tag bytes. None of these is zero, deliberately: a run of zero bytes — a
// zero-filled page, a short read padded out, a buffer that was never written — must
// not decode as a valid outcome or a valid replay result. `Sat` in particular would
// be the worst possible default. Enforced by `tests/package.rs::
// a_zero_byte_is_not_a_valid_outcome` and `..._not_a_valid_replay`.
const OUTCOME_SAT: u8 = 1;
const OUTCOME_UNSAT: u8 = 2;
const OUTCOME_UNKNOWN: u8 = 3;
const REPLAY_REPRODUCED: u8 = 1;
const REPLAY_REFUSED: u8 = 2;
const REPLAY_NOT_ATTEMPTED: u8 = 3;
const ABSENT: u8 = 0;
const PRESENT: u8 = 1;

/// One recorded solver query, as it survives a round trip through a file.
///
/// The name is a `String` rather than the `&'static str` of
/// [`Query`](crate::verification::Query) for the plain reason that a name read out
/// of a file is not static. See [`Manifest::queries`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordedQuery {
    pub name: String,
    pub required: Outcome,
    pub returned: Outcome,
}

/// One recorded difficulty measurement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordedMeasurement {
    pub name: String,
    pub value: u32,
}

/// What happened when the witness was replayed, as it survives a round trip.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecordedReplay {
    Reproduced(StateHash),
    Refused(String),
    NotAttempted,
}

impl RecordedReplay {
    #[must_use]
    pub fn reproduced(&self) -> bool {
        matches!(self, RecordedReplay::Reproduced(_))
    }
}

/// Everything a package carries about one puzzle.
///
/// This is the body of the file, in the order it is written. Building one by hand is
/// how a package is written; getting one back is [`open`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Manifest {
    /// Which family, by its stable id. A reader that does not have this family
    /// installed must refuse the package rather than attempt it, which is the
    /// registry's job and not this module's.
    pub family: String,
    /// Which version of that family's semantics. A replay against different
    /// semantics is not a replay.
    pub ruleset_version: u32,
    pub generator_version: u32,
    /// The seed, so the puzzle can be rebuilt from scratch and re-checked.
    pub seed: u64,
    /// What the uniqueness claim is relative to. Carried into the file because
    /// uniqueness is only ever true relative to an encoding, and a stored artifact
    /// that did not say which encoding would be claiming more than was proved.
    pub encoding: String,
    /// The level the game assigned, if any. `None` is a real answer and is stored
    /// as one.
    pub band: Option<String>,
    /// The family's own canonical bytes for the puzzle itself: for Nonogram, the
    /// clues. The platform does not interpret these. It is the family that knows
    /// what they mean, and keeping them opaque here is what stops the engine
    /// growing an opinion about a family's content.
    pub payload: Vec<u8>,
    /// Every query and what it returned, in the order they were asked.
    ///
    /// # The friction worth naming
    ///
    /// These cannot be handed to [`crate::verification::decide`], because that
    /// function's [`Query`](crate::verification::Query) names are `&'static str` and
    /// a name read out of a file is not static. So a package read back from disk
    /// cannot currently be re-judged by the one gate, which matters for the first of
    /// the four jobs in `content-delivery.md` section 6: re-verifying the corpus when
    /// the rules or the solver change.
    ///
    /// This module does **not** paper over that with a second copy of the gate's
    /// logic, which would be the one thing worse than the gap. Resolving it means
    /// changing the gate's signature, which is the most load-bearing function in the
    /// crate, so it is recorded in ADR 0003's not-enforced list and left as a
    /// decision rather than taken quietly here.
    pub queries: Vec<RecordedQuery>,
    pub measurements: Vec<RecordedMeasurement>,
    pub replay: RecordedReplay,
}

impl Manifest {
    /// Build a manifest from a report and the family's puzzle bytes.
    ///
    /// The report's `content_id` is deliberately *not* copied in. A package's content
    /// id is the digest of the body this manifest becomes, so taking one from the
    /// report would be a second, independently derived copy of the same value —
    /// exactly the failure the content module exists to prevent. [`seal`] computes it.
    #[must_use]
    pub fn from_report(report: &ValidationReport, payload: Vec<u8>) -> Self {
        Manifest {
            family: report.ruleset.family.0.to_owned(),
            ruleset_version: report.ruleset.version,
            generator_version: report.generator_version,
            seed: report.seed,
            encoding: report.encoding.to_owned(),
            band: report.band.map(str::to_owned),
            payload,
            queries: report.queries.iter().map(RecordedQuery::from).collect(),
            measurements: report
                .measurements
                .iter()
                .map(RecordedMeasurement::from)
                .collect(),
            replay: RecordedReplay::from(report.replay),
        }
    }

    /// Look up one measurement by name. `None` when it was not recorded, which is a
    /// different fact from a measurement of zero.
    #[must_use]
    pub fn measurement(&self, name: &str) -> Option<u32> {
        self.measurements
            .iter()
            .find(|m| m.name == name)
            .map(|m| m.value)
    }

    /// Whether any recorded query came back undecided.
    ///
    /// Reading this off a stored package is one of the four jobs the report serves.
    /// It is a question about what is written down, not a judgement about whether the
    /// puzzle may be published, which is why it is here and a `publishable` is not.
    #[must_use]
    pub fn has_undecided_query(&self) -> bool {
        self.queries.iter().any(|q| !q.returned.is_decisive())
    }

    /// The manifest's canonical bytes: the single definition of this manifest as a
    /// byte string.
    ///
    /// Every variable-length part is written as a length followed by its contents, so
    /// two different manifests cannot encode alike. An over-long field is an error
    /// rather than a truncation, because a silently shortened length is how two
    /// manifests come to share a fingerprint.
    pub fn canonical_bytes(&self) -> Result<Vec<u8>, EncodeError> {
        let mut out = Vec::new();
        put_str(&mut out, &self.family, "family")?;
        out.extend_from_slice(&self.ruleset_version.to_le_bytes());
        out.extend_from_slice(&self.generator_version.to_le_bytes());
        out.extend_from_slice(&self.seed.to_le_bytes());
        put_str(&mut out, &self.encoding, "encoding")?;
        match &self.band {
            None => out.push(ABSENT),
            Some(b) => {
                out.push(PRESENT);
                put_str(&mut out, b, "band")?;
            }
        }
        put_bytes32(&mut out, &self.payload, "payload")?;

        out.extend_from_slice(&count16(self.queries.len(), "queries")?.to_le_bytes());
        for q in &self.queries {
            put_str(&mut out, &q.name, "query name")?;
            out.push(outcome_tag(q.required));
            out.push(outcome_tag(q.returned));
        }

        out.extend_from_slice(&count16(self.measurements.len(), "measurements")?.to_le_bytes());
        for m in &self.measurements {
            put_str(&mut out, &m.name, "measurement name")?;
            out.extend_from_slice(&m.value.to_le_bytes());
        }

        match &self.replay {
            RecordedReplay::Reproduced(h) => {
                out.push(REPLAY_REPRODUCED);
                out.extend_from_slice(&h.0.to_le_bytes());
            }
            RecordedReplay::Refused(reason) => {
                out.push(REPLAY_REFUSED);
                put_str(&mut out, reason, "replay refusal reason")?;
            }
            RecordedReplay::NotAttempted => out.push(REPLAY_NOT_ATTEMPTED),
        }

        Ok(out)
    }
}

impl From<&Query> for RecordedQuery {
    fn from(q: &Query) -> Self {
        RecordedQuery {
            name: q.name.to_owned(),
            required: q.required,
            returned: q.returned,
        }
    }
}

impl From<&Measurement> for RecordedMeasurement {
    fn from(m: &Measurement) -> Self {
        RecordedMeasurement {
            name: m.name.to_owned(),
            value: m.value,
        }
    }
}

impl From<Replay> for RecordedReplay {
    fn from(r: Replay) -> Self {
        match r {
            Replay::Reproduced(h) => RecordedReplay::Reproduced(h),
            Replay::Refused(reason) => RecordedReplay::Refused(reason.to_owned()),
            Replay::NotAttempted => RecordedReplay::NotAttempted,
        }
    }
}

/// Signs a content id.
///
/// **No implementation in this crate.** Signing needs a private key and a reviewed
/// implementation, and a core that could sign would be a core that could forge. The
/// signature covers the content id, which covers the body, so signing the id signs
/// the whole package.
pub trait Signer {
    /// Which key this is, so a verifier can find the matching public key. Written
    /// into the file next to the signature.
    fn key_id(&self) -> &str;
    fn sign(&self, content_id: &ContentId) -> Vec<u8>;
}

/// Checks a signature over a content id.
///
/// **No implementation in this crate**, for the same reason as [`Signer`]: a crate
/// that verifies its own signatures proves nothing.
///
/// The `key_id` is handed over rather than used to look up a key here, and an
/// implementation **must** refuse a key it does not trust. The key id travels in the
/// file and the signature does not cover it, so anyone can put their own id and
/// their own signature on a package they built; what stops that package being
/// accepted is a verifier that knows which keys are allowed to sign. A verifier that
/// trusts whatever key the file names verifies nothing.
pub trait SignatureVerifier {
    fn verify(&self, content_id: &ContentId, key_id: &str, signature: &[u8]) -> bool;
}

/// Why a manifest could not be written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EncodeError {
    /// A field is longer than the format can express. Never silently shortened.
    TooLong { field: &'static str, len: usize },
}

/// Why a package could not be opened. Every variant is a refusal to proceed; there
/// is no variant meaning "opened, with reservations".
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpenError {
    /// The magic bytes are absent. Not a package at all.
    NotAPackage,
    /// A container version this reader does not know. Refused rather than guessed at.
    UnsupportedFormat(u16),
    /// The file ends in the middle of something.
    Truncated { field: &'static str },
    /// Bytes after the end of the package. A reader that ignored them would accept
    /// two different files as the same package.
    TrailingBytes(usize),
    /// The digest of the body is not the content id the file states. The file has
    /// been changed, or was written by something that computes identity differently.
    ///
    /// The stated value is carried as raw bytes, not as a [`ContentId`]. A
    /// `ContentId` can only be made by digesting bytes — that is the whole of the
    /// content module's discipline — and these bytes are a claim found in a file
    /// rather than something this code digested. Giving them the type would make
    /// the claim and the fact indistinguishable.
    ContentIdMismatch {
        stated: [u8; CONTENT_ID_LEN],
        computed: ContentId,
    },
    /// The verifier rejected the signature over the content id. Carries nothing: a
    /// rejected signature has nothing to tell the caller about why.
    SignatureRejected,
    /// A field inside the body is not well-formed — an unknown tag, or text that is
    /// not UTF-8.
    Malformed { field: &'static str },
}

/// A package whose bytes have been vouched for.
///
/// The private field is load-bearing: it means no code outside this module can build
/// one, so the only way to hold an `Opened` is to have gone through [`open`] with a
/// digest and a verifier. A caller cannot decode a manifest and claim it was checked.
///
/// This refuses to compile:
///
/// ```compile_fail
/// use gs_core::content::{ContentId, Digest};
/// use gs_core::package::{Manifest, Opened, RecordedReplay};
///
/// struct D;
/// impl Digest for D {
///     fn digest(&self, _bytes: &[u8]) -> [u8; 32] { [0; 32] }
/// }
///
/// let manifest = Manifest {
///     family: "nonogram".to_owned(),
///     ruleset_version: 1,
///     generator_version: 1,
///     seed: 0,
///     encoding: String::new(),
///     band: None,
///     payload: Vec::new(),
///     queries: Vec::new(),
///     measurements: Vec::new(),
///     replay: RecordedReplay::NotAttempted,
/// };
/// // Every field is named, including the private one, and every value is
/// // well-typed. The error is that the field is private, which is the point:
/// // this claim cannot be made from outside the module. Naming the field is
/// // what makes this test mean something — leaving it out would fail as a
/// // missing field whether it were private or not.
/// let _forged = Opened {
///     content_id: ContentId::of(&D, b"whatever I like"),
///     key_id: "a key I chose".to_owned(),
///     manifest,
///     checked_by_open: (),
/// };
/// ```
///
/// The same code without the final struct literal does compile, which is what says
/// the failure above is the private field and not a mistake somewhere in the setup:
///
/// ```
/// use gs_core::content::{ContentId, Digest};
/// use gs_core::package::{Manifest, RecordedReplay};
///
/// struct D;
/// impl Digest for D {
///     fn digest(&self, _bytes: &[u8]) -> [u8; 32] { [0; 32] }
/// }
///
/// let manifest = Manifest {
///     family: "nonogram".to_owned(),
///     ruleset_version: 1,
///     generator_version: 1,
///     seed: 0,
///     encoding: String::new(),
///     band: None,
///     payload: Vec::new(),
///     queries: Vec::new(),
///     measurements: Vec::new(),
///     replay: RecordedReplay::NotAttempted,
/// };
/// let _id = ContentId::of(&D, b"whatever I like");
/// let _bytes = manifest.canonical_bytes().expect("encodes");
/// ```
// Clippy offers `#[non_exhaustive]` here, and it is not the same thing.
// `#[non_exhaustive]` stops the rest of the *world* building one of these; a private
// field stops the rest of this *crate* too. The claim being protected is "these bytes
// were checked", and `open` is the only code entitled to make it, so the narrower
// guarantee is the one worth having.
#[allow(clippy::manual_non_exhaustive)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Opened {
    /// The fingerprint of the body, recomputed here rather than taken on trust.
    pub content_id: ContentId,
    /// Which key signed it. The verifier has already accepted this key.
    pub key_id: String,
    pub manifest: Manifest,
    checked_by_open: (),
}

/// Write a package: canonical body, its digest, and a signature over that digest.
pub fn seal<D: Digest, S: Signer>(
    manifest: &Manifest,
    digest: &D,
    signer: &S,
) -> Result<Vec<u8>, EncodeError> {
    let body = manifest.canonical_bytes()?;
    let content_id = ContentId::of(digest, &body);
    let signature = signer.sign(&content_id);
    let key_id = signer.key_id();

    let mut out = Vec::new();
    out.extend_from_slice(&MAGIC);
    out.extend_from_slice(&FORMAT_VERSION.to_le_bytes());
    put_bytes32(&mut out, &body, "body")?;
    out.extend_from_slice(content_id.as_bytes());
    put_str(&mut out, key_id, "key id")?;
    put_bytes32(&mut out, &signature, "signature")?;
    Ok(out)
}

/// Read a package, checking before decoding.
///
/// The order of operations is the contract:
///
/// 1. Framing — magic, version, and the lengths that say where the body ends.
/// 2. The digest of the body is recomputed and compared with the stated content id.
/// 3. The signature over that content id is put to the verifier.
/// 4. Only now is the body decoded.
///
/// So the body's parsers are never reached by bytes nobody vouched for. Enforced by
/// `tests/package.rs::a_body_that_fails_its_digest_is_never_decoded` and
/// `..._a_rejected_signature_is_never_decoded`.
pub fn open<D: Digest, V: SignatureVerifier>(
    bytes: &[u8],
    digest: &D,
    verifier: &V,
) -> Result<Opened, OpenError> {
    let mut r = Reader::new(bytes);

    if r.take(MAGIC.len(), "magic")? != MAGIC {
        return Err(OpenError::NotAPackage);
    }
    let version = r.u16("format version")?;
    if version != FORMAT_VERSION {
        return Err(OpenError::UnsupportedFormat(version));
    }

    let body = r.bytes32("body")?;
    let stated: [u8; CONTENT_ID_LEN] = r
        .take(CONTENT_ID_LEN, "content id")?
        .try_into()
        .expect("slice of exactly CONTENT_ID_LEN bytes");
    let key_id = r.string("key id")?;
    let signature = r.bytes32("signature")?;
    let remaining = r.remaining();
    if remaining != 0 {
        return Err(OpenError::TrailingBytes(remaining));
    }

    let computed = ContentId::of(digest, body);
    if computed.as_bytes() != &stated {
        return Err(OpenError::ContentIdMismatch { stated, computed });
    }
    // The signature is checked against the id this code computed, not the one the
    // file stated. They are equal by the check above, and using the computed one
    // means a future edit cannot accidentally leave a signature being verified
    // against a number the file supplied.
    if !verifier.verify(&computed, &key_id, signature) {
        return Err(OpenError::SignatureRejected);
    }

    let manifest = decode_manifest(body)?;
    Ok(Opened {
        content_id: computed,
        key_id,
        manifest,
        checked_by_open: (),
    })
}

fn decode_manifest(body: &[u8]) -> Result<Manifest, OpenError> {
    let mut r = Reader::new(body);
    let family = r.string("family")?;
    let ruleset_version = r.u32("ruleset version")?;
    let generator_version = r.u32("generator version")?;
    let seed = r.u64("seed")?;
    let encoding = r.string("encoding")?;
    let band = match r.u8("band presence")? {
        ABSENT => None,
        PRESENT => Some(r.string("band")?),
        _ => {
            return Err(OpenError::Malformed {
                field: "band presence",
            })
        }
    };
    let payload = r.bytes32("payload")?.to_vec();

    let query_count = r.u16("query count")?;
    let mut queries = Vec::with_capacity(usize::from(query_count));
    for _ in 0..query_count {
        let name = r.string("query name")?;
        let required = outcome_of(r.u8("required outcome")?)?;
        let returned = outcome_of(r.u8("returned outcome")?)?;
        queries.push(RecordedQuery {
            name,
            required,
            returned,
        });
    }

    let measurement_count = r.u16("measurement count")?;
    let mut measurements = Vec::with_capacity(usize::from(measurement_count));
    for _ in 0..measurement_count {
        let name = r.string("measurement name")?;
        let value = r.u32("measurement value")?;
        measurements.push(RecordedMeasurement { name, value });
    }

    let replay = match r.u8("replay tag")? {
        REPLAY_REPRODUCED => RecordedReplay::Reproduced(StateHash(r.u64("replay state hash")?)),
        REPLAY_REFUSED => RecordedReplay::Refused(r.string("replay refusal reason")?),
        REPLAY_NOT_ATTEMPTED => RecordedReplay::NotAttempted,
        _ => {
            return Err(OpenError::Malformed {
                field: "replay tag",
            })
        }
    };

    let remaining = r.remaining();
    if remaining != 0 {
        return Err(OpenError::TrailingBytes(remaining));
    }

    Ok(Manifest {
        family,
        ruleset_version,
        generator_version,
        seed,
        encoding,
        band,
        payload,
        queries,
        measurements,
        replay,
    })
}

fn outcome_tag(o: Outcome) -> u8 {
    match o {
        Outcome::Sat => OUTCOME_SAT,
        Outcome::Unsat => OUTCOME_UNSAT,
        Outcome::Unknown => OUTCOME_UNKNOWN,
    }
}

/// Decode an outcome tag.
///
/// An unknown tag is [`OpenError::Malformed`], never `Outcome::Unknown`. The two
/// would be easy to conflate and the difference is the whole discipline: a byte
/// nobody can read is a broken file, while `Outcome::Unknown` is a solver that
/// genuinely did not decide. Writing one down as the other would put a fabricated
/// solver result into an audit record.
fn outcome_of(tag: u8) -> Result<Outcome, OpenError> {
    match tag {
        OUTCOME_SAT => Ok(Outcome::Sat),
        OUTCOME_UNSAT => Ok(Outcome::Unsat),
        OUTCOME_UNKNOWN => Ok(Outcome::Unknown),
        _ => Err(OpenError::Malformed {
            field: "outcome tag",
        }),
    }
}

fn put_str(out: &mut Vec<u8>, s: &str, field: &'static str) -> Result<(), EncodeError> {
    put_bytes16(out, s.as_bytes(), field)
}

fn put_bytes16(out: &mut Vec<u8>, b: &[u8], field: &'static str) -> Result<(), EncodeError> {
    let len = u16::try_from(b.len()).map_err(|_| EncodeError::TooLong {
        field,
        len: b.len(),
    })?;
    out.extend_from_slice(&len.to_le_bytes());
    out.extend_from_slice(b);
    Ok(())
}

fn put_bytes32(out: &mut Vec<u8>, b: &[u8], field: &'static str) -> Result<(), EncodeError> {
    let len = u32::try_from(b.len()).map_err(|_| EncodeError::TooLong {
        field,
        len: b.len(),
    })?;
    out.extend_from_slice(&len.to_le_bytes());
    out.extend_from_slice(b);
    Ok(())
}

fn count16(n: usize, field: &'static str) -> Result<u16, EncodeError> {
    u16::try_from(n).map_err(|_| EncodeError::TooLong { field, len: n })
}

/// A cursor that cannot read past the end of its slice.
///
/// Every read is length-checked and returns [`OpenError::Truncated`] naming the field
/// it was reading, so a short file says where it stopped rather than panicking.
struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Reader { bytes, at: 0 }
    }

    fn remaining(&self) -> usize {
        self.bytes.len().saturating_sub(self.at)
    }

    fn take(&mut self, n: usize, field: &'static str) -> Result<&'a [u8], OpenError> {
        let end = self
            .at
            .checked_add(n)
            .ok_or(OpenError::Truncated { field })?;
        let slice = self
            .bytes
            .get(self.at..end)
            .ok_or(OpenError::Truncated { field })?;
        self.at = end;
        Ok(slice)
    }

    fn u8(&mut self, field: &'static str) -> Result<u8, OpenError> {
        Ok(self.take(1, field)?[0])
    }

    fn u16(&mut self, field: &'static str) -> Result<u16, OpenError> {
        let b: [u8; 2] = self.take(2, field)?.try_into().expect("two bytes");
        Ok(u16::from_le_bytes(b))
    }

    fn u32(&mut self, field: &'static str) -> Result<u32, OpenError> {
        let b: [u8; 4] = self.take(4, field)?.try_into().expect("four bytes");
        Ok(u32::from_le_bytes(b))
    }

    fn u64(&mut self, field: &'static str) -> Result<u64, OpenError> {
        let b: [u8; 8] = self.take(8, field)?.try_into().expect("eight bytes");
        Ok(u64::from_le_bytes(b))
    }

    fn bytes16(&mut self, field: &'static str) -> Result<&'a [u8], OpenError> {
        let len = usize::from(self.u16(field)?);
        self.take(len, field)
    }

    fn bytes32(&mut self, field: &'static str) -> Result<&'a [u8], OpenError> {
        let len = usize::try_from(self.u32(field)?).map_err(|_| OpenError::Truncated { field })?;
        self.take(len, field)
    }

    fn string(&mut self, field: &'static str) -> Result<String, OpenError> {
        let b = self.bytes16(field)?;
        core::str::from_utf8(b)
            .map(str::to_owned)
            .map_err(|_| OpenError::Malformed { field })
    }
}
