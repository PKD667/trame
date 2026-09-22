// The host lowering, against a workload this crate invented: `#[parallel]` is the list in order on
// the calling thread, `#[concurrent]` is one scoped thread per item, and both run every item
// before answering with the first `Err` in list order.

use std::sync::Barrier;

use crate::{Invoked, Keyed};

#[derive(Clone, Copy)]
struct Hit {
    cell: usize,
    amount: i64,
}

struct Rig;

impl Rig {
    #[crate::parallel]
    fn note(&self, hit: Hit, seen: &mut Vec<i64>) -> Result<(), i64> {
        seen.push(hit.amount);
        if hit.amount < 0 { Err(hit.amount) } else { Ok(()) }
    }

    #[crate::parallel]
    #[crate::ordered(key = hit.cell: usize)]
    fn charge(&self, hit: Hit, cell: &mut i64, charges: &mut usize) -> Result<(), ()> {
        *cell += hit.amount;
        *charges += 1;
        Ok(())
    }

    #[crate::concurrent]
    fn meet(&self, who: u32, all: &Barrier) -> Result<(), u32> {
        all.wait();
        if who == 0 { Ok(()) } else { Err(who) }
    }
}

#[test]
fn parallel_runs_the_list_in_order_and_answers_with_its_first_err() {
    let hits = [5, -1, 7, -2].map(|amount| Hit { cell: 0, amount });
    let mut seen = Vec::new();
    assert_eq!(crate::invoke!(Rig.note, &mut seen, &hits), Err(-1));
    assert_eq!(seen, [5, -1, 7, -2], "an Err does not stop the items after it");
}

#[test]
fn an_ordered_call_reaches_the_slot_its_key_names() {
    let hits = [(0, 3), (1, 5), (0, 7)].map(|(cell, amount)| Hit { cell, amount });
    let mut cells = [0i64; 2];
    let mut charges = 0;
    let rig = Rig;
    crate::invoke!(rig.charge, &mut charges, &hits, Keyed::new(&mut cells[..])).expect("in range");
    assert_eq!((cells, charges), ([10, 5], 3));

    let astray = [Hit { cell: 2, amount: 1 }, Hit { cell: 0, amount: 1 }];
    assert_eq!(
        crate::invoke!(rig.charge, &mut charges, &astray, Keyed::new(&mut cells[..])),
        Err(Invoked::OutOfRange { key: 2, len: 2 })
    );
    assert_eq!((cells, charges), ([11, 5], 4), "the item after the refused one ran");
}

#[test]
fn concurrent_runs_every_item_at_once_and_answers_in_list_order() {
    // Three parties at one barrier: run one after another, the first would wait forever.
    let all = Barrier::new(3);
    assert_eq!(crate::invoke!(Rig.meet, &all, &[0, 2, 1]), Err(2));
}
