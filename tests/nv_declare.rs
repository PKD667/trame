// `#[parallel]` under `nv`, on the host model of the warp: the list is split across the calling
// warp's 32 lanes, and an `#[ordered]` key is always on one lane, in issue order.

use core::cell::Cell;

use crate::Keyed;
use crate::nv::warp::{self, LANES};

#[derive(Clone, Copy)]
struct Step {
    cell: usize,
    by: i64,
}

struct Counter {
    cells: [Cell<i64>; 4],
}

impl Counter {
    #[crate::parallel]
    fn apply(&self, step: Step, applied: &mut usize) -> Result<(), ()> {
        let cell = &self.cells[step.cell];
        cell.set(cell.get() + step.by);
        *applied += 1;
        Ok(())
    }

    #[crate::parallel]
    #[crate::ordered(key = step.cell: usize)]
    fn record(&self, step: Step, trail: &mut Vec<(u32, i64)>, _: &mut ()) -> Result<(), ()> {
        trail.push((warp::lane(), step.by));
        Ok(())
    }
}

#[test]
fn the_list_is_partitioned_rather_than_repeated() {
    // Three steps, 32 lanes: a replication would apply each 32 times.
    let counter = Counter {
        cells: core::array::from_fn(|_| Cell::new(0)),
    };
    let steps = [(0, 3), (1, 5), (0, 7)].map(|(cell, by)| Step { cell, by });
    let applied = warp::sim(|| {
        let mut applied = 0;
        crate::invoke!(counter.apply, &mut applied, &steps).expect("no step fails");
        applied
    });
    assert_eq!(counter.cells.each_ref().map(Cell::get), [10, 5, 0, 0]);
    assert_eq!(applied[..4], [1, 1, 1, 0]);
}

#[test]
fn a_key_stays_on_one_lane_in_issue_order() {
    let counter = Counter {
        cells: core::array::from_fn(|_| Cell::new(0)),
    };
    let keys = [0, 33, 1, 0, 33, 32];
    let steps: Vec<Step> = (0..).zip(keys).map(|(by, cell)| Step { cell, by }).collect();
    let mut trails = vec![Vec::new(); 34];
    warp::sim(|| {
        crate::invoke!(counter.record, &mut (), &steps, Keyed::new(&mut trails[..]))
            .expect("every key in range")
    });
    let lane = |key: usize| (key % LANES as usize) as u32;
    assert_eq!(trails[0], [(lane(0), 0), (lane(0), 3)]);
    assert_eq!(trails[33], [(lane(33), 1), (lane(33), 4)]);
    assert_eq!(trails[1], [(lane(1), 2)]);
    assert_eq!(trails[32], [(lane(32), 5)]);
}
