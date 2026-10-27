//! `concurrent!` refusals that happen at run time, before any step: the cells and processes must
//! match (spec.md 5.3), and no tag may have two owners (spec.md 3). Each check aborts before a worker
//! effect, so each test runs on the test thread and checks the message. A primitive cell type is
//! refused at expansion instead: that is the compile_fail doc test in lib.rs.

extern crate std;

use super::tests_concurrent::Gate;
use super::tests_concurrent_io::Job;
use crate::status::Status;

/// A cell of its own type, that no process takes.
pub(super) struct Spare;

struct Solo;

#[crate::process]
impl Solo {
    fn step(&mut self, _g: &mut Gate) -> Status {
        Status::Done
    }
}

/// Two parameters, so it needs two cells.
struct Pair;

#[crate::process]
impl Pair {
    fn step(&mut self, _g: &mut Gate, _s: &mut Spare) -> Status {
        Status::wait()
    }
}

struct Own1;

#[crate::process(receives = Job)]
impl Own1 {
    fn step(&mut self, _g: &mut Gate) -> Status {
        Status::Done
    }
}

struct Own2;

#[crate::process(receives = Job)]
impl Own2 {
    fn step(&mut self, _g: &mut Gate) -> Status {
        Status::Done
    }
}

#[test]
#[should_panic(expected = "two cells have the type `")]
fn two_cells_of_one_type_abort_at_start() {
    let mut a = Gate { inside: 0, open: false };
    let mut b = Gate { inside: 0, open: false };
    let _ = crate::concurrent!(Solo; a, b);
}

#[test]
#[should_panic(expected = "is taken by no process's step; remove it from concurrent!")]
fn a_cell_no_step_takes_aborts_at_start() {
    let mut a = Gate { inside: 0, open: false };
    let mut s = Spare;
    let _ = crate::concurrent!(Solo; a, s);
}

#[test]
#[should_panic(expected = "has 2 cell parameters but 1 of them have a cell")]
fn a_step_parameter_without_a_cell_aborts_at_start() {
    let mut a = Gate { inside: 0, open: false };
    let _ = crate::concurrent!(Pair; a);
}

#[test]
#[should_panic(expected = "is owned by processes `")]
fn two_processes_owning_one_tag_abort_at_start() {
    let mut a = Gate { inside: 0, open: false };
    let _ = crate::concurrent!(Own1, Own2; a);
}
