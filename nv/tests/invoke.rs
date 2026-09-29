// Inline invocation: effects occur once, and repeated keys retain issue order.

use crate::Keyed;
use crate::sync::atomic::{AtomicU32, Ordering::Relaxed};
use crate::sync::{Exclusive, with};

#[derive(Clone, Copy)]
struct Step {
    cell: usize,
    by: i64,
}

struct Counter {
    cells: Exclusive<[i64; 4]>,
}

impl Counter {
    #[crate::parallel]
    fn apply(&self, step: Step, applied: &AtomicU32) {
        with(&self.cells, |cells| cells[step.cell] += step.by).expect("one inline caller");
        applied.fetch_add(1, Relaxed);
    }

    #[crate::parallel]
    #[crate::ordered(key = step.cell: usize)]
    fn record(&self, step: Step, trail: &mut Vec<i64>, _: &()) {
        trail.push(step.by);
    }
}

#[test]
fn the_list_is_run_once_rather_than_repeated() {
    let counter = Counter { cells: Exclusive::new([0; 4]) };
    let steps = [(0, 3), (1, 5), (0, 7)].map(|(cell, by)| Step { cell, by });
    let applied = AtomicU32::new(0);
    crate::invoke!(counter.apply, &applied, &steps).expect("no step fails");
    crate::invoke!(counter.apply, &applied, &[]).expect("empty input succeeds");
    assert_eq!(counter.cells.into_inner(), [10, 5, 0, 0]);
    assert_eq!(applied.load(Relaxed), 3);
}

#[test]
fn a_key_retains_issue_order_in_its_one_slot() {
    let counter = Counter { cells: Exclusive::new([0; 4]) };
    let keys = [0, 33, 1, 0, 33, 32];
    let steps: Vec<Step> = (0..).zip(keys).map(|(by, cell)| Step { cell, by }).collect();
    let mut trails = vec![Vec::new(); 34];
    crate::invoke!(counter.record, &(), &steps, Keyed::new(&mut trails[..]))
        .expect("every key in range");
    assert_eq!(trails[0], [0, 3]);
    assert_eq!(trails[33], [1, 4]);
    assert_eq!(trails[1], [2]);
    assert_eq!(trails[32], [5]);
}
