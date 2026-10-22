//! The platform suite, and the guard that stops it passing for nothing.
//!
//! CI runs this suite with **no puzzle family installed** (see
//! `.github/workflows/ci.yml`). A suite that runs with nothing installed can pass by
//! doing nothing, which is worse than not having it, so the list of checks has
//! exactly one definition — [`PLATFORM_CHECKS`] — and [`Ledger::assert_complete`]
//! fails unless every name in that list was recorded as having run.
//!
//! Adding a check means adding its name here. Forgetting to run it is then a
//! failure, not a silence.

pub const CHECK_EMPTY_REGISTRY_HAS_NO_FAMILIES: &str = "empty_registry_has_no_families";
pub const CHECK_UNKNOWN_FAMILY_IS_AN_ERROR: &str = "unknown_family_is_an_error";
pub const CHECK_UNKNOWN_IS_NOT_A_PROOF: &str = "unknown_is_not_a_proof";
pub const CHECK_UNKNOWN_IS_NOT_PUBLISHABLE: &str = "unknown_is_not_publishable";
pub const CHECK_EMPTY_GATE_REFUSES: &str = "empty_gate_refuses";
pub const CHECK_GATE_IS_A_PURE_FUNCTION: &str = "gate_is_a_pure_function";
pub const CHECK_CONTENT_ID_IS_BOUND_TO_BYTES: &str = "content_id_is_bound_to_bytes";
pub const CHECK_DSL_RESERVES_A_GLOBAL_TIER: &str = "dsl_reserves_a_global_tier";

/// Every check the platform suite must run. The single definition.
pub const PLATFORM_CHECKS: &[&str] = &[
    CHECK_EMPTY_REGISTRY_HAS_NO_FAMILIES,
    CHECK_UNKNOWN_FAMILY_IS_AN_ERROR,
    CHECK_UNKNOWN_IS_NOT_A_PROOF,
    CHECK_UNKNOWN_IS_NOT_PUBLISHABLE,
    CHECK_EMPTY_GATE_REFUSES,
    CHECK_GATE_IS_A_PURE_FUNCTION,
    CHECK_CONTENT_ID_IS_BOUND_TO_BYTES,
    CHECK_DSL_RESERVES_A_GLOBAL_TIER,
];

/// Records which checks actually ran.
#[derive(Default)]
pub struct Ledger {
    ran: Vec<&'static str>,
}

impl Ledger {
    #[must_use]
    pub fn new() -> Self {
        Ledger::default()
    }

    /// Record that `name` ran. Panics if `name` is not in [`PLATFORM_CHECKS`], so a
    /// check cannot drift away from its declared name.
    pub fn record(&mut self, name: &'static str) {
        assert!(
            PLATFORM_CHECKS.contains(&name),
            "{name} is not declared in PLATFORM_CHECKS"
        );
        assert!(
            !self.ran.contains(&name),
            "{name} recorded twice; each check records once"
        );
        self.ran.push(name);
    }

    /// The anti-vacuity guard.
    ///
    /// Fails if the declared list is empty, or if any declared check did not run.
    pub fn assert_complete(&self) {
        assert!(
            !PLATFORM_CHECKS.is_empty(),
            "the platform suite declares no checks; it would pass vacuously"
        );
        let missing: Vec<&&str> = PLATFORM_CHECKS
            .iter()
            .filter(|name| !self.ran.contains(*name))
            .collect();
        assert!(
            missing.is_empty(),
            "platform suite ran {} of {} declared checks; missing: {missing:?}",
            self.ran.len(),
            PLATFORM_CHECKS.len()
        );
    }
}
