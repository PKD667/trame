//! The parsing of TRAME_SEQ_STEP_LIMIT (limit.rs). The exit itself is tested by validate's mutants
//! check, which runs the built programs with the limit set.

extern crate std;

use super::limit::{parse, EXIT};

#[test]
fn unset_is_no_limit() {
    assert_eq!(parse(None), Ok(None));
}

#[test]
fn a_number_of_steps_is_the_limit() {
    assert_eq!(parse(Some("1")), Ok(Some(1)));
    assert_eq!(parse(Some("100000")), Ok(Some(100_000)));
}

#[test]
fn zero_empty_and_non_numbers_are_refused() {
    for bad in ["0", "", "-1", "3.5", "many"] {
        assert!(parse(Some(bad)).is_err(), "{bad:?} must be refused");
    }
}

#[test]
fn the_limit_exit_is_not_a_program_exit() {
    assert_ne!(EXIT, 0);
    assert_ne!(EXIT, 101);
}
