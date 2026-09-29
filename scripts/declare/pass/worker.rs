// The accepted grammar, each form once, run through `invoke!`.

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
    fn total(&self, hit: Hit, sum: &mut u64) -> Result<(), ()> {
        *sum += hit.amount;
        Ok(())
    }

    #[parallel]
    #[trame::ordered(key = hit.at.0: usize)]
    fn charge(&self, hit: Hit, cell: &mut u64, charges: &mut u32) -> Result<(), ()> {
        *cell += hit.amount;
        *charges += 1;
        Ok(())
    }

    pub fn run(&self, hits: &[Hit], cells: &mut [u64]) -> Result<(), Invoked<()>> {
        let mut sum = 0;
        invoke!(self.total, &mut sum, hits).map_err(Invoked::Failed)?;
        invoke!(self.charge, &mut 0, hits, Keyed::new(cells))?;
        let total = Exclusive::new(sum);
        let (zero, one) = (0u8, 1u8);
        let mut count = Count::<u8> { left: 0, at: &zero, other: &one };
        concurrent!(Add(&total), &mut count).map_err(Invoked::Failed)
    }
}
