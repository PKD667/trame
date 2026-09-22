// The accepted grammar, each form once, run through `invoke!`.

use std::sync::atomic::{AtomicU32, Ordering};

use trame::{Invoked, Keyed, concurrent, invoke, parallel};

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

    #[concurrent]
    fn role(&self, role: u8, seen: &AtomicU32) -> Result<(), u8> {
        seen.fetch_add(1 << role, Ordering::Relaxed);
        Ok(())
    }

    pub fn run(&self, hits: &[Hit], cells: &mut [u64]) -> Result<(), Invoked<()>> {
        let mut sum = 0;
        invoke!(self.total, &mut sum, hits).map_err(Invoked::Failed)?;
        invoke!(self.charge, &mut 0, hits, Keyed::new(cells))?;
        let seen = AtomicU32::new(0);
        invoke!(self.role, &seen, &[0, 1]).map_err(|_| Invoked::Failed(()))
    }
}
