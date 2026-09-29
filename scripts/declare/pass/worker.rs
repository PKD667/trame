// The accepted grammar, each form once, run through `invoke!`.

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering::Relaxed};

use trame::sync::{Exclusive, with};
use trame::{Invoked, Keyed, Step, concurrent, invoke, parallel, process};

#[derive(Clone, Copy)]
pub struct Hit {
    pub at: (usize, u8),
    pub amount: u64,
}

pub struct Worker;

#[process]
struct Add<'a>(&'a Exclusive<u64>);

impl Add<'_> {
    fn step(&mut self) -> Result<Step, ()> {
        match with(self.0, |t| *t += 1) {
            Ok(()) => Ok(Step::Done),
            Err(_) => Ok(Step::Idle),
        }
    }
}

// Lifetimes with bounds, a defaulted type and const parameter, and a `where` clause all reach the
// declaration's impl, defaults stripped.
#[process]
pub struct Count<'a, 'b: 'a, T: Copy + Into<u32> = u8, const N: u32 = 2>
where
    T: Send,
{
    left: u32,
    at: &'a T,
    other: &'b T,
}

impl<T: Copy + Into<u32> + Send, const N: u32> Count<'_, '_, T, N> {
    fn step(&mut self) -> Result<Step, ()> {
        self.left += 1;
        Ok(if self.left == N + (*self.at).into() { Step::Done } else { Step::Progress })
    }
}

impl Worker {
    #[parallel]
    fn total(&self, hit: Hit, sum: &AtomicU64) -> Result<(), ()> {
        sum.fetch_add(hit.amount, Relaxed);
        Ok(())
    }

    #[parallel]
    #[trame::ordered(key = hit.at.0: usize)]
    fn charge(&self, hit: Hit, cell: &mut u64, charges: &AtomicU32) -> Result<(), ()> {
        *cell += hit.amount;
        charges.fetch_add(1, Relaxed);
        Ok(())
    }

    // Returning nothing is the same as returning `Ok(())`: the function cannot fail, and may return early.
    #[parallel]
    #[trame::ordered(key = hit.at.0: usize)]
    fn add(&self, hit: Hit, cell: &mut u64, _: &()) {
        if hit.amount == 0 {
            return;
        }
        *cell += hit.amount;
    }

    #[parallel]
    fn note(&self, _: Hit, _: &()) {}

    pub fn run(&self, hits: &[Hit], cells: &mut [u64]) -> Result<(), Invoked<()>> {
        let sum = AtomicU64::new(0);
        invoke!(self.total, &sum, hits).map_err(Invoked::Failed)?;
        invoke!(self.charge, &AtomicU32::new(0), hits, Keyed::new(cells))?;
        let Ok(()) = invoke!(self.note, &(), hits);
        if let Err(Invoked::OutOfRange { .. }) = invoke!(self.add, &(), hits, Keyed::new(cells)) {
            return Err(Invoked::Failed(()));
        }
        let total = Exclusive::new(sum.into_inner());
        let (zero, one) = (0u8, 1u8);
        let mut count = Count::<u8> { left: 0, at: &zero, other: &one };
        concurrent!(Add(&total), &mut count).map_err(Invoked::Failed)
    }
}
