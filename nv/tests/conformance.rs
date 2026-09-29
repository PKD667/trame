// The conformance claims (`trame/conformance/claims.rs`) on nv's host model. The body touches only
// the public surface; what is here is the launcher's part, which writes the launch description.
//
// Ignored, because each claim's verdict is a JSON line that `trame/scripts/conform.sh` reads, and
// `M5` must run as its own invocation so a hang in `done` is attributed to it:
//
//     cargo test -p trame --features nv --lib nv::tests::conformance::claims -- --ignored --exact --nocapture

#[path = "../../conformance/claims.rs"]
mod claims;

use crate::{Environment, Launch};

/// Workers in nv's host model launch: enough for every claim to have distinct peers.
const WORKERS: u32 = 4;

/// nv: four warps and their one leader at launch rank 4. A second host of one worker, launch rank
/// 5, is in the table and nothing runs it: it makes a `Remote` address in range, so A1 meets nv's
/// `Unimplemented`. Each participant is a host thread, which
/// the model's `Context` permits, and discovers the launch as `contract.rs`'s do: the description
/// this thread was given and its warp's index. Links are eight deep, room for the lane depth one
/// `affected` implies. The leader's region is always provisioned, because every deployment has a
/// leader; `led` says whether a leader thread runs on it.
fn launch(led: bool, body: impl Fn(Environment, &[&[Launch]], u16, Launch) -> bool + Sync) -> bool {
    use crate::nv::launch::{self, Description};
    use crate::nv::layout::Layout;
    use crate::nv::leader::Route;
    use crate::nv::peers::Fabric;
    use crate::nv::{MAX_FRAME, warp};

    let workers: Vec<Launch> = (0..WORKERS).map(Launch::new).collect();
    let remote = [Launch::new(WORKERS + 1)];
    let hosts: [&[Launch]; 2] = [&workers, &remote];
    let leader = Launch::new(WORKERS);
    let route = Route::sized(WORKERS).expect("a route for the workers");
    let mut arena = vec![0u32; route.words()];
    route.init(&mut arena);
    let layout = Layout::new(8, MAX_FRAME as u32).expect("a valid layout");
    let description = Description::stated(
        WORKERS,
        Some(leader),
        Fabric::new(WORKERS, layout),
        arena.as_mut_ptr(),
        route.words(),
    );
    let (hosts, body) = (&hosts[..], &body);
    std::thread::scope(|scope| {
        let ranks: Vec<_> = (0..WORKERS)
            .map(|w| {
                let description = description.clone();
                scope.spawn(move || {
                    launch::describe(description);
                    warp::sim_warp(w, || body(Environment::default(), hosts, 0, leader))
                })
            })
            .collect();
        let leader = led.then(|| {
            let description = description.clone();
            scope.spawn(move || {
                launch::describe(description);
                claims::leader(Environment::default(), leader, hosts, 0)
            })
        });
        let all = ranks.into_iter().fold(true, |ok, w| ok & w.join().expect("a worker does not panic"));
        all & leader.is_none_or(|l| l.join().expect("the leader does not panic"))
    })
}

#[test]
#[ignore = "run by trame/scripts/conform.sh"]
fn claims() {
    assert!(launch(true, claims::worker), "a claim failed");
}

#[test]
#[ignore = "run by trame/scripts/conform.sh"]
fn pressure() {
    assert!(launch(false, claims::pressure), "a claim failed");
}
