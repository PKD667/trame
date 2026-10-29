//! `parallel!` (spec.md 5.4) against a reference fold. Needs `--cfg trame_checked`, as every use of
//! `parallel!` does: without it this file does not compile, by design.

use std::panic::{AssertUnwindSafe, catch_unwind};

/// One input: the slot it folds into, and its value.
#[derive(Clone, Copy)]
struct Work {
    key: u32,
    value: u64,
}

/// The shared context of the items.
struct Ctx {
    salt: u64,
}

const MUL: u64 = 1_000_003;

/// A fold that depends on the order of its inputs, so an out-of-order run changes the answer.
#[trame::item(key = x.key as usize)]
fn fold(x: &Work, slot: &mut u64, cx: &Ctx) {
    *slot = slot.wrapping_mul(MUL).wrapping_add(x.value ^ cx.salt);
}

/// A fixed-seed xorshift64: the same inputs on every run and every machine.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

/// The reference: for each slot, scan every input in order and fold the ones with its key. It
/// shares no code with the runtime's counting sort.
fn reference(items: &[Work], slots: &mut [u64], cx: &Ctx) {
    for (k, slot) in slots.iter_mut().enumerate() {
        for x in items {
            if x.key as usize == k {
                fold(x, slot, cx);
            }
        }
    }
}

#[test]
fn parallel_equals_the_reference_fold_on_1000_random_inputs() {
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    let mut total_items = 0;
    for case in 0..1000 {
        let n_slots = 1 + rng.below(8) as usize;
        let n_items = rng.below(40) as usize;
        let items: Vec<Work> =
            (0..n_items).map(|_| Work { key: rng.below(n_slots as u64) as u32, value: rng.next() }).collect();
        let start: Vec<u64> = (0..n_slots).map(|_| rng.next()).collect();
        let cx = Ctx { salt: rng.next() };

        let mut want = start.clone();
        reference(&items, &mut want, &cx);
        let mut got = start;
        trame::parallel!(fold, &cx, &items, &mut got);
        assert_eq!(got, want, "case {case}: {n_items} items, {n_slots} slots");
        total_items += n_items;
    }
    assert!(total_items > 10_000, "the cases exercised repeated keys: {total_items} items");
}

#[test]
fn a_key_without_a_slot_aborts_before_any_item_runs() {
    let cx = Ctx { salt: 1 };
    // The first two inputs are valid and would change their slots if they ran.
    let items =
        [Work { key: 0, value: 5 }, Work { key: 1, value: 6 }, Work { key: 9, value: 7 }, Work { key: 0, value: 8 }];
    let mut slots = [10_u64, 20];
    let r = catch_unwind(AssertUnwindSafe(|| trame::parallel!(fold, &cx, &items, &mut slots)));
    let msg = r.expect_err("a key without a slot must abort");
    let msg = msg.downcast_ref::<String>().expect("a formatted message");
    assert!(msg.contains("key 9") && msg.contains("2 slots"), "the message names the key and the count: {msg}");
    assert_eq!(slots, [10, 20], "no slot changed");
}

#[test]
fn no_items_and_no_slots_is_an_empty_fold() {
    let cx = Ctx { salt: 1 };
    let items: [Work; 0] = [];
    let mut slots: [u64; 0] = [];
    trame::parallel!(fold, &cx, &items, &mut slots);
}

/// Items of one key run in input order: a fold of three values, in two orders, differs.
#[test]
fn items_of_one_key_run_in_input_order() {
    let cx = Ctx { salt: 0 };
    let items = [Work { key: 0, value: 1 }, Work { key: 0, value: 2 }, Work { key: 0, value: 3 }];
    let mut slots = [0_u64];
    trame::parallel!(fold, &cx, &items, &mut slots);
    let in_order = [1_u64, 2, 3].iter().fold(0_u64, |s, v| s.wrapping_mul(MUL).wrapping_add(*v));
    assert_eq!(slots, [in_order]);
}
