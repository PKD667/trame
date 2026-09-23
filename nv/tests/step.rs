//! `concurrent!` on one warp: round-robin in source order, one step per live arm per turn.

use std::sync::Mutex;

use crate::Step;

fn arm(name: char, steps: u32, trail: &Mutex<Vec<char>>, taken: &mut u32) -> Result<Step, char> {
    trail.lock().expect("no step panics").push(name);
    *taken += 1;
    Ok(if *taken == steps { Step::Done } else { Step::Idle })
}

#[test]
fn arms_alternate_in_source_order_and_a_done_arm_is_never_stepped_again() {
    let trail = Mutex::new(Vec::new());
    let (mut a, mut b, mut c) = (0, 0, 0);
    crate::concurrent! {
        || arm('a', 2, &trail, &mut a),
        || arm('b', 4, &trail, &mut b),
        || arm('c', 3, &trail, &mut c),
    }
    .expect("no arm fails");
    let trail: String = trail.into_inner().expect("no step panics").into_iter().collect();
    assert_eq!(trail, "abcabcbcb");
}

#[test]
fn an_error_ends_the_turn_before_the_next_arm_steps() {
    let trail = Mutex::new(Vec::new());
    let mut b = 0;
    let answer = crate::concurrent! {
        || Err::<Step, char>('a'),
        || arm('b', 4, &trail, &mut b),
    };
    assert_eq!(answer, Err('a'));
    assert_eq!(b, 0);
}
