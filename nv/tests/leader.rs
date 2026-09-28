//! The leader route, both ends, on the host model.

use crate::contract::{Deployment, Error, Launch, Rank, Tag};
use crate::nv::Environment;
use crate::nv::launch::{self, Description};
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
///
/// The calling thread is the model's launcher as well as the leader: it writes the description
/// the leader then discovers.
fn open(region: *mut u32, workers: &[u32]) -> Leader {
    let size = workers.len() as u32;
    let workers: Vec<Launch> = workers.iter().copied().map(Launch::new).collect();
    let leaders = vec![Launch::new(size); workers.len()];
    let route = Route::sized(size).expect("a route for these workers");
    launch::describe(Description::stated(
        size,
        Some(Launch::new(size)),
        Fabric::new(size, Layout::new(DEPTH, CAPACITY).expect("a valid layout")),
        region,
        route.words(),
    ));
    Leader::open(
        Environment::default(),
        Deployment::new(&workers, Some(&leaders)).expect("a valid deployment"),
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
                let mut buf = vec![0u8; CAPACITY as usize];
                if let Some(frame) = leader.recv(&mut buf).expect("a frame") {
                    let len = frame.len();
                    heard.push((frame.source().expect("a worker sent it"), frame.tag(), buf[..len].to_vec()));
                }
            }
            heard.sort();
            // Answer each worker once, and only after both have been heard from: the down
            // links fill independently, so this also shows one worker's link does not block
            // the other's.
            for (rank, tag, _) in &heard {
                leader
                    .send(*rank, Tag::new(tag.get() + 100), b"ack")
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
                    let mut buf = vec![0u8; CAPACITY as usize];
                    // A spin, because the leader answers both workers only after hearing from
                    // both, and there is no device to yield to in this model.
                    loop {
                        match unsafe { worker.recv(&mut buf) } {
                            Ok(m) => break (m.tag, buf[..m.len as usize].to_vec()),
                            Err(refused) => assert_eq!(refused, RecvError::Empty),
                        }
                    }
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

    let r = Rank::from_index;
    assert_eq!(
        up,
        vec![
            (r(0), Tag::new(7), b"hello".to_vec()),
            (r(1), Tag::new(8), b"hello".to_vec())
        ]
    );
    assert_eq!(down, vec![(107, b"ack".to_vec()), (108, b"ack".to_vec())]);
}

#[test]
fn a_full_link_refuses_and_an_empty_one_reports() {
    let route = Route::sized(1).expect("one worker");
    let mut arena = vec![0u32; route.words()];
    route.init(&mut arena);
    let leader = open(arena.as_mut_ptr(), &[0]);
    let mut buf = vec![0u8; CAPACITY as usize];
    let mut worker = unsafe { Worker::new(arena.as_mut_ptr(), route, 0) };
    let zero = Rank::from_index(0);
    let tag = |n: u32| Tag::new(n as u16);

    // Nothing has been sent, so the leader is told so rather than made to wait.
    assert!(leader.recv(&mut buf).expect("empty").is_none());
    assert_eq!(unsafe { worker.recv(&mut buf) }, Err(RecvError::Empty));

    for n in 0..DEPTH {
        leader
            .send(zero, tag(n), b"x")
            .expect("room while the link is not full");
    }
    assert_eq!(leader.send(zero, tag(DEPTH), b"x"), Err(Error::Full));

    // The refusal consumed nothing: the first frame the worker takes is still the first one.
    let first = unsafe { worker.recv(&mut buf) }.expect("a frame");
    assert_eq!((first.tag, first.len), (0, 1));
    leader
        .send(zero, tag(DEPTH), b"x")
        .expect("the worker's receive made room");

    let mut small = [0u8; 0];
    assert!(matches!(
        unsafe { worker.recv(&mut small) },
        Err(RecvError::TooSmall { needed: 1 })
    ));
    // And a too-small output consumed nothing either, so the same frame is still there.
    let second = unsafe { worker.recv(&mut buf) }.expect("a frame");
    assert_eq!((second.tag, second.len), (1, 1));
}

#[test]
fn a_partial_word_is_copied_by_its_byte_count() {
    let route = Route::sized(1).expect("one worker");
    let mut arena = vec![0u32; route.words()];
    route.init(&mut arena);
    let leader = open(arena.as_mut_ptr(), &[0]);
    let mut worker = unsafe { Worker::new(arena.as_mut_ptr(), route, 0) };
    leader
        .send(Rank::from_index(0), Tag::new(1), b"hello")
        .expect("an empty link");
    // Five bytes into six: the frame's last word is partial, and the byte past it is the caller's.
    let mut out = [0xa5u8; 6];
    let got = unsafe { worker.recv(&mut out) }.expect("a frame");
    assert_eq!(got.len, 5);
    assert_eq!(out, *b"hello\xa5");
}
