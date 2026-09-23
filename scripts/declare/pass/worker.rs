// The accepted grammar, each form once, run through `invoke!`.

use trame::sync::Exclusive;
use trame::{Invoked, Keyed, Step, concurrent, invoke, parallel};

#[derive(Clone, Copy)]
pub struct Hit {
    pub at: (usize, u8),
    pub amount: u64,
}

pub struct Worker;

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
        let mut left = 2;
        concurrent! {
            || match total.with(|t| *t += 1) {
                Ok(()) => Ok(Step::Done),
                Err(_) => Ok(Step::Idle),
            },
            || {
                left -= 1;
                Ok(if left == 0 { Step::Done } else { Step::Progress })
            },
        }
        .map_err(Invoked::Failed)
    }
}
