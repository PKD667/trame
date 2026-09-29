// The same pass, twice: once as an `#[ordered]` `#[parallel]` function run through `invoke!`, once
// written by hand with the same answers — every item runs, a key naming no cell is
// `Invoked::OutOfRange`, the first `Err` in list order is returned. `check.sh --lowering` compiles
// this with optimisation, cuts both functions out of the assembly and compares them.

use std::convert::Infallible;

use trame::{Invoked, Keyed};

#[derive(Clone, Copy)]
pub struct Hit {
    pub target: usize,
    pub charge: u64,
}

#[inline(always)]
fn update(cell: &mut u64, charge: u64) {
    *cell = cell.wrapping_add(charge).rotate_left(7) ^ charge;
}

#[trame::parallel]
#[trame::ordered(key = hit.target: usize)]
fn charge(hit: Hit, cell: &mut u64, _: &()) -> Result<(), Infallible> {
    update(cell, hit.charge);
    Ok(())
}

#[unsafe(no_mangle)]
pub extern "C" fn declared_body(cells: *mut u64, n: usize, hits: *const Hit, m: usize) -> bool {
    let (cells, hits) = unsafe { (std::slice::from_raw_parts_mut(cells, n), std::slice::from_raw_parts(hits, m)) };
    trame::invoke!(charge, &(), hits, Keyed::new(cells)).is_ok()
}

#[unsafe(no_mangle)]
pub extern "C" fn handwritten_body(cells: *mut u64, n: usize, hits: *const Hit, m: usize) -> bool {
    let (cells, hits) = unsafe { (std::slice::from_raw_parts_mut(cells, n), std::slice::from_raw_parts(hits, m)) };
    let len = cells.len();
    let mut first: Result<(), Invoked<Infallible>> = Ok(());
    for &hit in hits {
        let outcome = match cells.get_mut(hit.target) {
            Some(cell) => {
                update(cell, hit.charge);
                Ok(())
            }
            None => Err(Invoked::OutOfRange { key: hit.target, len }),
        };
        if first.is_ok() {
            first = outcome;
        }
    }
    first.is_ok()
}
