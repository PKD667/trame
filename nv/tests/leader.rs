//! The leader route, both ends, on the host model.

use crate::contract::{Deployment, Error, Frame, Rank, Wait};
use crate::nv::Environment;
use crate::nv::error::RecvError;
use crate::nv::layout::Layout;
use crate::nv::leader::{CAPACITY, DEPTH, Leader, Route, Worker};
use crate::nv::peers::Fabric;

/// The region lives in this test's own storage, which is the one thing a host run can supply
/// that a launch supplies itself. Every other line below is the same code a device run uses on
/// the worker side of the host model.
struct Region(*mut u32);

// SAFETY: the region is one allocation and the endpoints below divide it by worker, with one
// producer and one consumer per link.
unsafe impl Send for Region {}
unsafe impl Sync for Region {}

/// The leader of `workers`, over `region`. The leader's rank is one past the last worker.
fn open(region: *mut u32, workers: &[Rank]) -> Leader {
    let size = workers.len() as u32;
    let leaders = vec![size; workers.len()];
    Leader::open(
        Environment {
            rank: size,
            size,
            fabric: Fabric::new(size, Layout::new(DEPTH, CAPACITY).expect("a valid layout")),
            segment: std::ptr::null_mut(),
            segment_bytes: 0,
            leader_region: region,
        },
        Deployment {
            workers,
            leaders: &leaders,
        },
    )
    .expect("a deployment with a leader")
}

#[test]
fn a_leader_and_its_workers_talk_both_ways() {
    let route = Route::sized(2).expect("two workers");
    let mut arena = vec![0u32; route.words()];
    route.init(&mut arena);
    let region = Region(arena.as_mut_ptr());

    let (up, down) = std::thread::scope(|scope| {
        let leader = scope.spawn(|| {
            let region = &region;
            let leader = open(region.0, &[0, 1]);
            let mut heard = Vec::new();
            // Two workers, so the leader must see both — an empty answer made after one check
            // would leave a job running that nobody was listening to.
            while heard.len() < 2 {
                let mut buf = [0u8; CAPACITY as usize];
                if let Some(Frame { source, tag, len }) =
                    leader.recv(&mut buf, Wait::Poll).expect("a frame")
                {
                    heard.push((source, tag, buf[..len as usize].to_vec()));
                }
            }
            heard.sort();
            // Answer each worker once, and only after both have been heard from: the down
            // links fill independently, so this also shows one worker's link does not block
            // the other's.
            for (rank, tag, _) in &heard {
                leader
                    .send(*rank, tag + 100, b"ack", Wait::Poll)
                    .expect("room for an ack");
            }
            heard
        });

        let workers: Vec<_> = (0..2u32)
            .map(|rank| {
                let region = &region;
                scope.spawn(move || {
                    let mut worker = unsafe { Worker::new(region.0, route, rank) };
                    unsafe { worker.send(rank + 7, b"hello") }.expect("an empty link");
                    let mut buf = [0u8; CAPACITY as usize];
                    let mut answer = None;
                    // A wait here is a spin because the leader answers both workers only after
                    // hearing from both, and there is no device to yield to in this model.
                    for _ in 0..100_000_000u64 {
                        if let Some((tag, len)) = unsafe { worker.recv(&mut buf) }.expect("an ack")
                        {
                            answer = Some((tag, buf[..len as usize].to_vec()));
                            break;
                        }
                    }
                    answer.expect("the leader answers")
                })
            })
            .collect();

        let heard = leader.join().expect("the leader does not panic");
        let answers: Vec<_> = workers
            .into_iter()
            .map(|w| w.join().expect("a worker"))
            .collect();
        (heard, answers)
    });

    assert_eq!(
        up,
        vec![(0, 7, b"hello".to_vec()), (1, 8, b"hello".to_vec())]
    );
    assert_eq!(down, vec![(107, b"ack".to_vec()), (108, b"ack".to_vec())]);
}

#[test]
fn a_full_link_refuses_and_an_empty_one_reports() {
    let route = Route::sized(1).expect("one worker");
    let mut arena = vec![0u32; route.words()];
    route.init(&mut arena);
    let leader = open(arena.as_mut_ptr(), &[0]);
    let mut buf = [0u8; CAPACITY as usize];
    let mut worker = unsafe { Worker::new(arena.as_mut_ptr(), route, 0) };

    // Nothing has been sent, so the leader is told so rather than made to wait.
    assert!(leader.recv(&mut buf, Wait::Poll).expect("empty").is_none());
    assert!(unsafe { worker.recv(&mut buf) }.expect("empty").is_none());

    for n in 0..DEPTH {
        leader
            .send(0, n, b"x", Wait::Poll)
            .expect("room while the link is not full");
    }
    assert_eq!(leader.send(0, DEPTH, b"x", Wait::Poll), Err(Error::Full));

    // The refusal consumed nothing: the first frame the worker takes is still the first one.
    assert_eq!(
        unsafe { worker.recv(&mut buf) }.expect("a frame"),
        Some((0u32, 1u32))
    );
    leader
        .send(0, DEPTH, b"x", Wait::Poll)
        .expect("the worker's receive made room");

    let mut small = [0u8; 0];
    assert!(matches!(
        unsafe { worker.recv(&mut small) },
        Err(RecvError::TooSmall { needed: 1 })
    ));
    // And a too-small output consumed nothing either, so the same frame is still there.
    assert_eq!(
        unsafe { worker.recv(&mut buf) }.expect("a frame"),
        Some((1u32, 1u32))
    );
}
