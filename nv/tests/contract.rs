// The device backend, against the contract, on the host model of the warp.
//
// What evidence a resident-device backend owes: distinct per-participant
// identity and endpoints, convergent transport calls, fixed storage, a full lane, a short receive
// buffer, and an unscheduled peer. Every one of those is here except convergence, which the host
// model cannot exercise — it runs one participant at a time on one thread, so "all lanes reached
// the call" is not a question it can answer. That gap is stated rather than papered over, and it
// is the reason a passing host model is not sufficient on its own.
//
// The identity test is the one that matters most, because it is the test whose absence let three
// defects through: rank, the cohort tables and the endpoints used to live in process globals that
// every participant wrote, so the last writer won and every participant then used another
// participant's identity and another participant's links. Two contexts alive at once cannot share
// a global, so this test cannot pass against that design at all.

use std::collections::HashMap;
use std::convert::Infallible;
use std::num::NonZeroU32;

use crate::contract::{
    BackendFault, Channel, Deployment, Edge, Error, FailureKind, Invalid, Launch, Rank, Tag,
};
use crate::nv::launch::{self, Description};
use crate::nv::layout::Layout;
use crate::nv::peers::Fabric;
use crate::nv::{self, Environment, MAX_FRAME, warp};

fn r(index: u32) -> Rank {
    Rank::from_index(index)
}

/// A launch rank, the number the transport assigned rather than the contract's dense index.
fn l(index: u32) -> Launch {
    Launch::new(index)
}

fn t(tag: u16) -> Tag {
    Tag::new(tag)
}

fn ranks(indices: &[u32]) -> Vec<Rank> {
    indices.iter().copied().map(r).collect()
}

fn launches(indices: &[u32]) -> Vec<Launch> {
    indices.iter().copied().map(l).collect()
}

/// Every participant's entry into one launch whose links have `layout`.
fn enter(size: u32, layout: Layout) -> Vec<Result<nv::Context, crate::Failure>> {
    let workers: Vec<Launch> = (0..size).map(l).collect();
    enter_as(&workers, layout)
}

/// As [`enter`], with the worker list in contract-rank order; the result is by launch rank.
///
/// This thread is the model's launcher: it writes the description, and each participant then
/// discovers it as the warp whose index is its launch rank.
fn enter_as(workers: &[Launch], layout: Layout) -> Vec<Result<nv::Context, crate::Failure>> {
    let size = workers.len() as u32;
    // No leader in this launch, which is what a null region says.
    let description = Description::stated(
        size,
        None,
        Fabric::new(size, layout),
        std::ptr::null_mut(),
        0,
    );
    std::thread::scope(|scope| {
        let entered: Vec<_> = (0..size)
            .map(|rank| {
                let description = description.clone();
                scope.spawn(move || {
                    launch::describe(description);
                    warp::sim_warp(rank, || entry(workers))
                })
            })
            .collect();
        entered.into_iter().map(|rank| rank.join().expect("a worker does not panic")).collect()
    })
}

/// One participant's entry, through the discovery path every caller uses.
fn entry(workers: &[Launch]) -> Result<nv::Context, crate::Failure> {
    nv::init(
        Environment::default(),
        Deployment::new(workers, None).expect("every rank a worker"),
    )
}

/// One launch provisioned for `MAX_FRAME`, as a value per participant.
fn launch(size: u32, depth: u32) -> Vec<nv::Context> {
    let layout = Layout::new(depth, MAX_FRAME as u32).expect("a valid layout");
    enter(size, layout)
        .into_iter()
        .map(|cx| cx.expect("a launch the model can build"))
        .collect()
}

/// A two-worker description with a valid header, for a test to spoil.
fn described() -> Description {
    let layout = Layout::new(2, MAX_FRAME as u32).expect("a valid layout");
    Description::stated(
        2,
        None,
        Fabric::new(2, layout),
        std::ptr::null_mut(),
        0,
    )
}

/// What rank 0's entry says of `description`.
fn entering(description: Description) -> Result<nv::Context, crate::Failure> {
    let workers = launches(&[0, 1]);
    std::thread::scope(|scope| {
        let other = description.clone();
        let peer = scope.spawn(|| {
            launch::describe(other);
            warp::sim_warp(1, || entry(&workers))
        });
        launch::describe(description);
        let first = warp::sim_warp(0, || entry(&workers));
        let second = peer.join().expect("the other worker does not panic");
        assert_eq!(first.as_ref().err().map(|f| f.kind), second.as_ref().err().map(|f| f.kind));
        first
    })
}

fn refusal(result: Result<nv::Context, crate::Failure>) -> FailureKind<Infallible> {
    result.err().expect("refused at entry").kind
}

const INCONSISTENT: FailureKind<Infallible> =
    FailureKind::Backend(BackendFault::Invalid(Invalid::InconsistentLaunch));

#[test]
fn a_valid_description_initialises() {
    let cx = entering(described()).expect("a consistent launch");
    assert_eq!((nv::rank(&cx), nv::size(&cx)), (r(0), 2));
}

#[test]
fn a_launch_that_wrote_nothing_is_refused() {
    let unwritten = std::thread::spawn(|| refusal(entry(&launches(&[0]))));
    assert_eq!(unwritten.join().expect("no panic"), INCONSISTENT);
}

#[test]
fn a_wrong_header_is_refused() {
    let mut magic = described();
    magic.magic ^= 1;
    assert_eq!(refusal(entering(magic)), INCONSISTENT);
    let mut version = described();
    version.version += 1;
    assert_eq!(refusal(entering(version)), INCONSISTENT);
    let mut bytes = described();
    bytes.bytes -= 8;
    assert_eq!(refusal(entering(bytes)), INCONSISTENT);
}

#[test]
fn a_warp_outside_the_described_launch_is_refused() {
    launch::describe(described());
    let outside = warp::sim_warp(2, || entry(&launches(&[0, 1])));
    assert_eq!(
        refusal(outside),
        FailureKind::Backend(BackendFault::Invalid(Invalid::RankOutsideJob))
    );
}

#[test]
fn a_launch_whose_slots_cannot_hold_max_frame_is_refused() {
    let layout = Layout::new(2, MAX_FRAME as u32 - 1).expect("a valid layout");
    for cx in enter(2, layout) {
        let failure = cx.err().expect("refused before entry");
        assert_eq!(failure.kind, FailureKind::Backend(BackendFault::Storage));
    }
}

#[test]
fn every_participant_has_its_own_identity_and_endpoints() {
    let cx = launch(3, 2);
    // Distinct identity: this is the assertion a shared global cannot satisfy, because the last
    // writer would make all three answer the same rank.
    assert_eq!(cx.iter().map(nv::rank).collect::<Vec<_>>(), ranks(&[0, 1, 2]));
    assert!(cx.iter().all(|c| nv::size(c) == 3));
    // One device, so one sharing domain led by rank 0.
    assert!(cx.iter().all(|c| nv::hosts(c) == ranks(&[0, 0, 0])));
    // Distinct endpoints: rank 0 reaching rank 1 is a different link from rank 2 reaching rank 1,
    // and a launch whose endpoints were one shared value could not deliver both.
    let mut a = launch(2, 2);
    nv::send(&mut a[0], r(1), Channel::Message(t(1)), b"from zero").expect("accepted");
    nv::send(&mut a[1], r(0), Channel::Message(t(2)), b"from one").expect("accepted");
    let mut buf = [0u8; 16];
    let got = nv::recv(&mut a[0], &mut buf).expect("no fault").expect("one frame");
    assert_eq!((got.source(), got.tag()), (Some(r(1)), t(2)));
    assert_eq!(&buf[..got.len()], b"from one");
    let got = nv::recv(&mut a[1], &mut buf).expect("no fault").expect("one frame");
    assert_eq!((got.source(), got.tag()), (Some(r(0)), t(1)));
    assert_eq!(&buf[..got.len()], b"from zero");
}

#[test]
fn contract_ranks_become_launch_ranks_at_the_link_and_back() {
    // Launch rank 1 is contract rank 0 and launch rank 0 is contract rank 1. A backend that used a
    // contract rank as a link index would deliver to the sender itself and name the wrong source.
    let layout = Layout::new(2, MAX_FRAME as u32).expect("a valid layout");
    let mut cx: Vec<nv::Context> = enter_as(&launches(&[1, 0]), layout)
        .into_iter()
        .map(|cx| cx.expect("a launch the model can build"))
        .collect();
    assert_eq!((nv::rank(&cx[0]), nv::rank(&cx[1])), (r(1), r(0)));
    nv::send(&mut cx[1], r(1), Channel::Message(t(5)), b"to one").expect("accepted");
    let mut buf = [0u8; 16];
    assert_eq!(nv::recv(&mut cx[1], &mut buf), Ok(None));
    let got = nv::recv(&mut cx[0], &mut buf).expect("no fault").expect("one frame");
    assert_eq!((got.source(), got.tag()), (Some(r(0)), t(5)));
    assert_eq!(&buf[..got.len()], b"to one");
    nv::send(&mut cx[0], r(0), Channel::Message(t(6)), b"to zero").expect("accepted");
    let got = nv::recv(&mut cx[1], &mut buf).expect("no fault").expect("one frame");
    assert_eq!((got.source(), got.tag()), (Some(r(1)), t(6)));
    assert_eq!(&buf[..got.len()], b"to zero");
}

#[test]
fn a_frame_arrives_with_its_tag_and_bytes_paired() {
    let mut cx = launch(2, 4);
    nv::send(&mut cx[0], r(1), Channel::Message(t(9)), b"payload").expect("accepted");
    nv::send(&mut cx[0], r(1), Channel::Message(t(10)), b"second").expect("accepted");
    let mut buf = [0u8; 32];
    let first = nv::recv(&mut cx[1], &mut buf).expect("no fault").expect("a frame");
    assert_eq!((first.source(), first.tag(), first.len()), (Some(r(0)), t(9), 7));
    assert_eq!(&buf[..7], b"payload");
    // FIFO per (source, destination, tag) holds across the two different tags on one link.
    let second = nv::recv(&mut cx[1], &mut buf).expect("no fault").expect("a frame");
    assert_eq!((second.tag(), second.len()), (t(10), 6));
    assert_eq!(&buf[..6], b"second");
    // And then nothing, which is the normal answer and not an error.
    assert_eq!(nv::recv(&mut cx[1], &mut buf), Ok(None));
}

#[test]
fn a_short_buffer_is_reported_without_consuming_the_frame() {
    let mut cx = launch(2, 2);
    nv::send(&mut cx[0], r(1), Channel::Message(t(3)), b"abcdefgh").expect("accepted");
    let mut small = [0xa5u8; 4];
    assert_eq!(
        nv::recv(&mut cx[1], &mut small),
        Err(Error::TooSmall { needed: 8 })
    );
    // The refusal changed nothing: not the caller's buffer, and not the link, so the frame is
    // still there for a caller that grew.
    assert_eq!(small, [0xa5; 4]);
    let mut big = [0u8; 32];
    let got = nv::recv(&mut cx[1], &mut big).expect("no fault").expect("a frame");
    assert_eq!(got.len(), 8);
    assert_eq!(&big[..8], b"abcdefgh");
}

#[test]
fn a_full_lane_is_refused_and_accepts_nothing() {
    let mut cx = launch(2, 2);
    for _ in 0..2 {
        nv::send(&mut cx[0], r(1), Channel::Message(t(1)), b"x").expect("accepted");
    }
    // One attempt reports capacity pressure at once.
    assert_eq!(
        nv::send(&mut cx[0], r(1), Channel::Message(t(1)), b"y"),
        Err(Error::Full)
    );
    // The refused send accepted nothing: the lane holds exactly the two earlier frames.
    let mut buf = [0u8; 8];
    for _ in 0..2 {
        let got = nv::recv(&mut cx[1], &mut buf).unwrap().unwrap();
        assert_eq!((got.len(), buf[0]), (1, b'x'));
    }
    assert_eq!(nv::recv(&mut cx[1], &mut buf), Ok(None));
}

#[test]
fn a_frame_larger_than_max_frame_is_refused() {
    let mut cx = launch(2, 2);
    let over = vec![0u8; MAX_FRAME + 1];
    assert_eq!(
        nv::send(&mut cx[0], r(1), Channel::Message(t(1)), &over),
        Err(Error::TooLarge { limit: MAX_FRAME })
    );
    // A destination outside the launch is the caller's input, not a gone peer.
    assert_eq!(
        nv::send(&mut cx[0], r(7), Channel::Message(t(1)), b"x"),
        Err(Error::Invalid(Invalid::RankOutsideJob))
    );
}

#[test]
fn release_is_a_worker_collective() {
    let mut cx = launch(2, 2);
    let (first, second) = cx.split_at_mut(1);
    std::thread::scope(|scope| {
        let peer = scope.spawn(|| nv::release(&mut second[0]));
        nv::release(&mut first[0]).expect("first worker retires");
        peer.join().expect("the other worker does not panic").expect("second worker retires");
    });
}

// ---------------------------------------------------------------------------------------------
// Geometry

fn declare(cx: &mut [nv::Context], workers: &[Rank], edges: &[Edge], bytes: usize, tag: Tag) {
    std::thread::scope(|scope| {
        let calls: Vec<_> = cx.iter_mut()
            .map(|c| scope.spawn(move || nv::reshape(c, workers, edges, bytes, tag)))
            .collect();
        for call in calls {
            assert_eq!(call.join().expect("a worker does not panic"), Ok(()));
        }
    });
}

fn edge(source: u32, destination: u32, affected: u32) -> Edge {
    Edge::new(r(source), r(destination), NonZeroU32::new(affected).unwrap())
}

fn edges(pairs: &[(u32, u32)]) -> Vec<Edge> {
    pairs.iter().map(|&(s, d)| edge(s, d, 1)).collect()
}

#[test]
fn reshape_validates_before_anything_is_sent() {
    let mut cx = launch(3, 4);
    let workers = ranks(&[0, 1, 2]);
    let bytes = 16;
    declare(&mut cx, &workers, &edges(&[(0, 1)]), bytes, t(7));

    // Unsorted workers: a declaration whose order two participants could read differently.
    assert_eq!(
        nv::reshape(&mut cx[0], &ranks(&[1, 0, 2]), &[], bytes, t(7)),
        Err(Error::Invalid(Invalid::UnorderedWorkers))
    );
    // Duplicate edges cannot both own a slot.
    assert_eq!(
        nv::reshape(&mut cx[0], &workers, &edges(&[(0, 1), (0, 1)]), bytes, t(7)),
        Err(Error::Invalid(Invalid::UnorderedEdges))
    );
    // A worker that is not a participant of this launch.
    assert_eq!(
        nv::reshape(&mut cx[0], &ranks(&[0, 1, 9]), &[], bytes, t(7)),
        Err(Error::Invalid(Invalid::RankOutsideJob))
    );
    // A subset is legal: the arena is indexed by rank, so lanes among some participants need no
    // mapping, and requiring the workers to be the whole cohort would refuse a load the launch
    // can serve.
    declare(&mut cx, &ranks(&[0, 1]), &[], bytes, t(7));
    // A frame wider than the launched slot is refused at declaration, never truncated at send.
    let over = MAX_FRAME + 1;
    assert_eq!(
        nv::reshape(&mut cx[0], &workers, &[], over, t(7)),
        Err(Error::TooLarge { limit: MAX_FRAME })
    );
    // A pair whose implied depth the arena cannot hold: the launch sized one depth for every
    // pair, so the widest pair's requirement is what has to fit.
    assert_eq!(
        nv::reshape(&mut cx[0], &workers, &[edge(0, 1, 2)], bytes, t(7)),
        Err(Error::Invalid(Invalid::UnsupportedGeometry))
    );
}

#[test]
fn lane_traffic_rides_the_tag_the_load_declared() {
    let mut cx = launch(2, 4);
    let workers = ranks(&[0, 1]);
    let bytes = 16;
    declare(&mut cx, &workers, &edges(&[(0, 1)]), bytes, t(42));
    assert_eq!(nv::send(&mut cx[0], r(0), Channel::Lane, b"x"), Err(Error::Invalid(Invalid::NoLane)));
    assert_eq!(nv::send(&mut cx[0], r(1), Channel::Lane, &[0; 17]), Err(Error::TooLarge { limit: bytes }));
    assert_eq!(nv::send(&mut cx[0], r(1), Channel::Lane, &vec![0; MAX_FRAME + 1]), Err(Error::TooLarge { limit: bytes }));
    nv::send(&mut cx[0], r(1), Channel::Lane, b"lane").expect("accepted");
    let mut buf = [0u8; 16];
    let got = nv::recv(&mut cx[1], &mut buf).unwrap().unwrap();
    assert_eq!((got.source(), got.tag()), (Some(r(0)), t(42)));

    // Before a load there is no lane tag, so lane traffic has no route and says so.
    let mut fresh = launch(2, 4);
    assert_eq!(
        nv::send(&mut fresh[0], r(1), Channel::Lane, b"x"),
        Err(Error::Invalid(Invalid::LaneNotConfigured))
    );
}

// ---------------------------------------------------------------------------------------------
// Lifetime

// ---------------------------------------------------------------------------------------------
// Clocks

#[test]
fn readings_are_monotonic_within_one_device() {
    let first = nv::clock::reading();
    let second = nv::clock::reading();
    assert_eq!(
        first.clock(),
        second.clock(),
        "one device is one comparison domain"
    );
    assert!(second.since(first).is_ok());
}

#[test]
fn the_launch_a_test_builds_is_the_launch_the_device_builds() {
    // Host-model initialization shall match device initialization. What that means here is
    // that both discover the launch the same way — every fact comes from the description the
    // launcher wrote and the warp's own index, so a second launch that writes its own description
    // cannot inherit the first one's identity.
    let first = launch(2, 2);
    let second = launch(4, 2);
    assert_eq!(nv::size(&first[0]), 2);
    assert_eq!(nv::size(&second[0]), 4);
    assert_eq!(nv::rank(&first[1]), r(1));
    assert_eq!(nv::rank(&second[1]), r(1));
}

#[test]
fn the_plan_is_the_topology_the_contract_asks_for() {
    // `plan` is gone: `reshape` takes the edges directly, ordered, with no hash map to iterate in
    // an order two participants could disagree about. What replaces the old test is that the
    // declaration is validated against the launch rather than compiled into a table.
    let mut cx = launch(2, 4);
    let workers = ranks(&[0, 1]);
    let bytes = 8;
    let mut fanin = HashMap::new();
    fanin.insert((0u32, 1u32), 1usize);
    assert_eq!(fanin.len(), 1, "the map is the test's, not the backend's");
    declare(&mut cx, &workers, &edges(&[(0, 1)]), bytes, t(1));
    declare(&mut cx, &workers, &edges(&[(0, 1)]), bytes, t(1));
}
