//! Round-robin in source order, one bounded step per live process per turn.

use std::sync::Mutex;

use crate::Step;

#[crate::process]
struct Count<'a> {
    name: char,
    steps: u32,
    trail: &'a Mutex<Vec<char>>,
    taken: &'a mut u32,
}

impl Count<'_> {
    fn step(&mut self) -> Result<Step, char> {
        self.trail.lock().expect("no step panics").push(self.name);
        *self.taken += 1;
        Ok(if *self.taken == self.steps { Step::Done } else { Step::Idle })
    }
}

#[crate::process]
struct Fails;

impl Fails {
    fn step(&mut self) -> Result<Step, char> {
        Err('a')
    }
}

#[test]
fn arms_alternate_in_source_order_and_a_done_arm_is_never_stepped_again() {
    let trail = Mutex::new(Vec::new());
    let (mut a, mut b, mut c) = (0, 0, 0);
    crate::concurrent!(
        Count { name: 'a', steps: 2, trail: &trail, taken: &mut a },
        Count { name: 'b', steps: 4, trail: &trail, taken: &mut b },
        Count { name: 'c', steps: 3, trail: &trail, taken: &mut c },
    )
    .expect("no arm fails");
    let trail: String = trail.into_inner().expect("no step panics").into_iter().collect();
    assert_eq!(trail, "abcabcbcb");
}

#[test]
fn an_error_ends_the_turn_before_the_next_arm_steps() {
    let trail = Mutex::new(Vec::new());
    let mut b = 0;
    let answer = crate::concurrent!(
        Fails,
        Count { name: 'b', steps: 4, trail: &trail, taken: &mut b },
    );
    assert_eq!(answer, Err('a'));
    assert_eq!(b, 0);
}
