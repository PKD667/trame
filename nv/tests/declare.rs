//! The warp lowering's answers on the host model: what the device example `nv-declared` runs on
//! hardware, and what a lane returns when one of its items fails or names no slot.

use core::cell::Cell;

use crate::nv::warp::{self, LANES};
use crate::{Invoked, Keyed};

struct Grid {
    visits: [Cell<u32>; 96],
}

impl Grid {
    #[crate::parallel]
    fn visit(&self, at: usize, lanes: &mut Vec<usize>) -> Result<(), usize> {
        self.visits[at].set(self.visits[at].get() + 1);
        lanes.push(at);
        if at % 40 == 39 { Err(at) } else { Ok(()) }
    }

    #[crate::parallel]
    #[crate::ordered(key = at: usize)]
    fn fill(&self, at: usize, slot: &mut u32, _: &mut ()) -> Result<(), ()> {
        *slot += at as u32 + 1;
        Ok(())
    }
}

#[test]
fn ninety_six_indices_over_thirty_two_lanes_run_once_each() {
    let grid = Grid {
        visits: core::array::from_fn(|_| Cell::new(0)),
    };
    let list: Vec<usize> = (0..96).collect();
    let answers = warp::sim(|| {
        let mut mine = Vec::new();
        (crate::invoke!(grid.visit, &mut mine, &list), mine)
    });
    assert!(grid.visits.iter().all(|v| v.get() == 1));
    for (lane, (answer, mine)) in answers.into_iter().enumerate() {
        assert_eq!(mine, [lane, lane + 32, lane + 64]);
        // Lane 7 carries 39 and 71; lane 15 carries 79. Each lane answers for its own items.
        let failed = mine.iter().copied().find(|at| at % 40 == 39);
        assert_eq!(answer, failed.map_or(Ok(()), Err));
    }
}

#[test]
fn a_key_out_of_range_is_refused_on_its_own_lane() {
    let grid = Grid {
        visits: core::array::from_fn(|_| Cell::new(0)),
    };
    let mut slots = [0u32; LANES as usize];
    let answers = warp::sim(|| {
        crate::invoke!(grid.fill, &mut (), &[40, 3], Keyed::new(&mut slots[..]))
    });
    assert_eq!(answers[8], Err(Invoked::OutOfRange { key: 40, len: 32 }));
    assert!(answers.iter().enumerate().all(|(lane, a)| lane == 8 || a.is_ok()));
    assert_eq!(slots[3], 4, "the item after the refused one ran");
}
