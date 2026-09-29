//! List-order effects and one owned outcome through the HEAD declaration surface.

use crate::sync::atomic::{AtomicU32, Ordering::Relaxed};
use crate::sync::{Exclusive, with};
use crate::{Invoked, Keyed};

struct Grid {
    visits: [AtomicU32; 96],
}

impl Grid {
    #[crate::parallel]
    fn visit(&self, at: usize, trail: &Exclusive<Vec<usize>>) -> Result<(), Box<usize>> {
        self.visits[at].fetch_add(1, Relaxed);
        with(trail, |trail| trail.push(at)).expect("one inline caller");
        if at % 40 == 39 { Err(Box::new(at)) } else { Ok(()) }
    }

    #[crate::parallel]
    #[crate::ordered(key = at: usize)]
    fn fill(&self, at: usize, slot: &mut u32, _: &()) {
        *slot += at as u32 + 1;
    }
}

#[test]
fn ninety_six_indices_run_once_in_list_order_with_one_first_error() {
    let grid = Grid {
        visits: core::array::from_fn(|_| AtomicU32::new(0)),
    };
    let list: Vec<usize> = (0..96).collect();
    let trail = Exclusive::new(Vec::new());
    assert_eq!(crate::invoke!(grid.visit, &trail, &list), Err(Box::new(39)));
    assert!(grid.visits.iter().all(|v| v.load(Relaxed) == 1));
    assert_eq!(trail.into_inner(), list, "items after both errors also ran");
}

#[test]
fn an_out_of_range_key_is_refused_and_later_repeated_keys_still_run() {
    let grid = Grid {
        visits: core::array::from_fn(|_| AtomicU32::new(0)),
    };
    let mut slots = [0u32; 32];
    assert_eq!(
        crate::invoke!(grid.fill, &(), &[40, 3, 3], Keyed::new(&mut slots[..])),
        Err(Invoked::OutOfRange { key: 40, len: 32 }),
    );
    assert_eq!(slots[3], 8, "both items after the refused one ran");
    assert!(slots.iter().enumerate().all(|(at, &value)| at == 3 || value == 0));
}
