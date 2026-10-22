//! The platform suite, run with **no puzzle family installed**.
//!
//! This is the day-one check described in `docs/adr/0003-engine-game-boundary.md`.
//! If a change to the platform only holds because Nonogram happens to be present,
//! it fails here.

use gs_core::content::{ContentId, Digest};
use gs_core::dsl::{Constraint, Global, Tier};
use gs_core::platform_suite::{
    Ledger, CHECK_CONTENT_ID_IS_BOUND_TO_BYTES, CHECK_DSL_RESERVES_A_GLOBAL_TIER,
    CHECK_EMPTY_GATE_REFUSES, CHECK_EMPTY_REGISTRY_HAS_NO_FAMILIES, CHECK_GATE_IS_A_PURE_FUNCTION,
    CHECK_UNKNOWN_FAMILY_IS_AN_ERROR, CHECK_UNKNOWN_IS_NOT_A_PROOF,
    CHECK_UNKNOWN_IS_NOT_PUBLISHABLE,
};
use gs_core::registry::{FamilyId, LookupError, Registry};
use gs_core::verification::{decide, Outcome, Query, Refusal};

/// A deliberately weak digest, for exercising the *binding* discipline only.
///
/// Not a hash function in any security sense, and it is in the test tree so that it
/// cannot be reached from anything that publishes. M4 replaces the real
/// implementation; see `content::Digest`.
struct CountingDigest;

impl Digest for CountingDigest {
    fn digest(&self, bytes: &[u8]) -> [u8; 32] {
        let mut out = [0u8; 32];
        for (i, b) in bytes.iter().enumerate() {
            out[i % 32] ^= b.rotate_left((i % 8) as u32);
        }
        out[31] ^= u8::try_from(bytes.len() % 251).unwrap_or(0);
        out
    }
}

#[test]
fn platform_holds_with_no_family_installed() {
    let mut ledger = Ledger::new();
    let registry = Registry::empty();

    assert_eq!(registry.installed_count(), 0);
    ledger.record(CHECK_EMPTY_REGISTRY_HAS_NO_FAMILIES);

    // No fallback, no default family, no silently-synthesised ruleset.
    let id = FamilyId("nonogram");
    assert_eq!(
        registry.resolve(id).err(),
        Some(LookupError::NotInstalled(id))
    );
    ledger.record(CHECK_UNKNOWN_FAMILY_IS_AN_ERROR);

    // A timeout is not a proof of unsatisfiability.
    assert!(!Outcome::Unknown.proves_unsatisfiable());
    assert!(Outcome::Unsat.proves_unsatisfiable());
    assert!(!Outcome::Sat.proves_unsatisfiable());
    ledger.record(CHECK_UNKNOWN_IS_NOT_A_PROOF);

    // An undecided query is a refusal, and is reported as undecided rather than as a
    // failure, so the report keeps SAT / UNSAT / UNKNOWN distinct.
    let undecided = [Query {
        name: "uniqueness",
        required: Outcome::Unsat,
        returned: Outcome::Unknown,
    }];
    assert_eq!(decide(&undecided), Err(Refusal::Undecided("uniqueness")));
    ledger.record(CHECK_UNKNOWN_IS_NOT_PUBLISHABLE);

    // A gate with nothing asserted against it refuses.
    assert_eq!(decide(&[]), Err(Refusal::Failed("no_queries_asserted")));
    ledger.record(CHECK_EMPTY_GATE_REFUSES);

    // The publication decision is a function of its inputs and nothing else.
    let queries = [
        Query {
            name: "solvable",
            required: Outcome::Sat,
            returned: Outcome::Sat,
        },
        Query {
            name: "uniqueness",
            required: Outcome::Unsat,
            returned: Outcome::Unsat,
        },
    ];
    assert_eq!(decide(&queries), Ok(()));
    assert_eq!(decide(&queries), decide(&queries));
    ledger.record(CHECK_GATE_IS_A_PURE_FUNCTION);

    // Identity comes from the bytes, and differing bytes differ in identity.
    let d = CountingDigest;
    let a = ContentId::of(&d, b"puzzle-a");
    let b = ContentId::of(&d, b"puzzle-b");
    assert_ne!(a, b);
    assert_eq!(a, ContentId::of(&d, b"puzzle-a"));
    assert_eq!(a.to_hex().len(), 64);
    ledger.record(CHECK_CONTENT_ID_IS_BOUND_TO_BYTES);

    // The DSL can express a Tier B global constraint with no family installed, and
    // reports it as global rather than as one more local rule. Slitherlink is family
    // two, and discovering this on family five with the DSL already frozen around
    // local constraints is the expensive version of this check.
    let loop_rule = Constraint::Global(Global::SingleClosedLoop { value: 1 });
    assert_eq!(loop_rule.tier(), Tier::B);
    assert_eq!(loop_rule.globals().len(), 1);
    ledger.record(CHECK_DSL_RESERVES_A_GLOBAL_TIER);

    ledger.assert_complete();
}
