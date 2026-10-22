//! The guard that stops the platform suite passing for nothing.
//!
//! `platform_with_no_family.rs` runs with an empty registry, which is exactly the
//! shape of suite that can pass by doing nothing. These tests check the guard that
//! prevents that, because an unchecked guard is not a guard.

use gs_core::platform_suite::{Ledger, CHECK_UNKNOWN_IS_NOT_A_PROOF, PLATFORM_CHECKS};

#[test]
fn the_suite_declares_at_least_one_check() {
    assert!(!PLATFORM_CHECKS.is_empty());
}

#[test]
#[should_panic(expected = "missing")]
fn a_ledger_that_ran_nothing_fails() {
    Ledger::new().assert_complete();
}

#[test]
#[should_panic(expected = "missing")]
fn a_ledger_missing_one_check_fails() {
    let mut ledger = Ledger::new();
    for name in PLATFORM_CHECKS.iter().skip(1) {
        ledger.record(name);
    }
    ledger.assert_complete();
}

#[test]
#[should_panic(expected = "not declared in PLATFORM_CHECKS")]
fn an_undeclared_check_name_is_rejected() {
    Ledger::new().record("a_check_nobody_declared");
}

#[test]
#[should_panic(expected = "recorded twice")]
fn one_check_cannot_stand_in_for_the_others() {
    let mut ledger = Ledger::new();
    ledger.record(CHECK_UNKNOWN_IS_NOT_A_PROOF);
    ledger.record(CHECK_UNKNOWN_IS_NOT_A_PROOF);
}
