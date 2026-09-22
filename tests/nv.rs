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
use std::num::NonZeroU32;

use crate::contract::{
    BackendFault, Channel, Deployment, Edge, Error, FailureKind, FrameBytes, Invalid, Rank, Tag,
};
use crate::nv::layout::Layout;
use crate::nv::peers::Fabric;
use crate::nv::{self, Environment, MAX_FRAME};

/// The segment every test's launch publishes into. Small: the partition rule is what is under
/// test, not the size.
const SEGMENT: usize = 64;

fn r(index: u32) -> Rank {
    Rank::from_index(index)
}

fn t(tag: u16) -> Tag {
    Tag::new(tag)
}

fn ranks(indices: &[u32]) -> Vec<Rank> {
    indices.iter().copied().map(r).collect()
}

/// Every participant's entry into one launch whose links have `layout`.
///
/// The segment is leaked because the launch owns it for its whole life and the contexts borrow it,
/// which is exactly the lifetime the device's arena has.
fn enter(size: u32, layout: Layout) -> Vec<Result<nv::Context, crate::Failure>> {
    let fabric = Fabric::new(size, layout);
    let segment: &'static mut [u8] = vec![0u8; SEGMENT].leak();
    let workers: Vec<Rank> = (0..size).map(r).collect();
    (0..size)
        .map(|rank| {
            nv::init(
                Environment {
                    rank: r(rank),
                    size,
                    fabric: fabric.clone(),
                    segment: segment.as_mut_ptr(),
                    segment_bytes: segment.len(),
                    // No leader in this launch, which is what a null region says.
                    leader_region: std::ptr::null_mut(),
                },
                Deployment::new(&workers, None).expect("every rank a worker"),
                // One device: every rank is in the one cohort, whatever colour the rule returns.
                |_, _| 0,
            )
        })
        .collect()
}

/// One launch provisioned for `MAX_FRAME`, as a value per participant.
fn launch(size: u32, depth: u32) -> Vec<nv::Context> {
    let layout = Layout::new(depth, MAX_FRAME.get()).expect("a valid layout");
    enter(size, layout)
        .into_iter()
        .map(|cx| cx.expect("a launch the model can build"))
        .collect()
}

#[test]
fn a_launch_whose_slots_cannot_hold_max_frame_is_refused() {
    let layout = Layout::new(2, MAX_FRAME.get() - 1).expect("a valid layout");
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
    assert_eq!((got.source(), got.tag()), (r(1), t(2)));
    assert_eq!(&buf[..got.len().get() as usize], b"from one");
    let got = nv::recv(&mut a[1], &mut buf).expect("no fault").expect("one frame");
    assert_eq!((got.source(), got.tag()), (r(0), t(1)));
    assert_eq!(&buf[..got.len().get() as usize], b"from zero");
}

#[test]
fn a_frame_arrives_with_its_tag_and_bytes_paired() {
    let mut cx = launch(2, 4);
    nv::send(&mut cx[0], r(1), Channel::Message(t(9)), b"payload").expect("accepted");
    nv::send(&mut cx[0], r(1), Channel::Message(t(10)), b"second").expect("accepted");
    let mut buf = [0u8; 32];
    let first = nv::recv(&mut cx[1], &mut buf).expect("no fault").expect("a frame");
    assert_eq!((first.source(), first.tag(), first.len().get()), (r(0), t(9), 7));
    assert_eq!(&buf[..7], b"payload");
    // FIFO per (source, destination, tag) holds across the two different tags on one link.
    let second = nv::recv(&mut cx[1], &mut buf).expect("no fault").expect("a frame");
    assert_eq!((second.tag(), second.len().get()), (t(10), 6));
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
        Err(Error::TooSmall {
            needed: FrameBytes::try_from(8).unwrap()
        })
    );
    // The refusal changed nothing: not the caller's buffer, and not the link, so the frame is
    // still there for a caller that grew.
    assert_eq!(small, [0xa5; 4]);
    let mut big = [0u8; 32];
    let got = nv::recv(&mut cx[1], &mut big).expect("no fault").expect("a frame");
    assert_eq!(got.len().get(), 8);
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
        assert_eq!((got.len().get(), buf[0]), (1, b'x'));
    }
    assert_eq!(nv::recv(&mut cx[1], &mut buf), Ok(None));
}

#[test]
fn a_frame_larger_than_max_frame_is_refused() {
    let mut cx = launch(2, 2);
    let over = vec![0u8; MAX_FRAME.get() as usize + 1];
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
fn local_release_discharges_the_callers_obligation() {
    let mut cx = launch(2, 2);
    nv::release(&mut cx[0]).expect("a local discharge cannot fail");
}

// ---------------------------------------------------------------------------------------------
// Geometry

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
    let bytes = FrameBytes::try_from(16).unwrap();
    assert_eq!(
        nv::reshape(&mut cx[0], &workers, &edges(&[(0, 1)]), bytes, t(7)),
        Ok(())
    );

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
    assert_eq!(nv::reshape(&mut cx[0], &ranks(&[0, 1]), &[], bytes, t(7)), Ok(()));
    // A frame wider than the launched slot is refused at declaration, never truncated at send.
    let over = FrameBytes::try_from(MAX_FRAME.get() as usize + 1).unwrap();
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
    let bytes = FrameBytes::try_from(16).unwrap();
    for c in cx.iter_mut() {
        nv::reshape(c, &workers, &edges(&[(0, 1)]), bytes, t(42)).expect("a valid declaration");
    }
    nv::send(&mut cx[0], r(1), Channel::Lane, b"lane").expect("accepted");
    let mut buf = [0u8; 16];
    let got = nv::recv(&mut cx[1], &mut buf).unwrap().unwrap();
    assert_eq!((got.source(), got.tag()), (r(0), t(42)));

    // Before a load there is no lane tag, so lane traffic has no route and says so.
    let mut fresh = launch(2, 4);
    assert_eq!(
        nv::send(&mut fresh[0], r(1), Channel::Lane, b"x"),
        Err(Error::Invalid(Invalid::LaneNotConfigured))
    );
}

// ---------------------------------------------------------------------------------------------
// Lifetime

#[test]
fn the_segment_is_the_union_of_the_slices() {
    let cx = launch(2, 2);
    // One device, one sharing domain, so both ranks are in the same partition — and a device has
    // no pages, so the split is even rather than page-rounded.
    let hosts = nv::hosts(&cx[0]);
    let domain = ranks(&[0, 1]);
    let half = SEGMENT / 2;
    let slice = |rank| crate::partition::slice_of(hosts, &domain, r(rank), SEGMENT).unwrap();
    assert_eq!((slice(0).offset, slice(0).length), (0, half));
    assert_eq!((slice(1).offset, slice(1).length), (half, half));
    let mut two = launch(2, 2);
    let shared = nv::share(&mut two[0], &[7u8; SEGMENT / 2], SEGMENT).expect("half the segment");
    assert_eq!(nv::bytes(&shared).len(), SEGMENT);
    assert_eq!(&nv::bytes(&shared)[..half], &[7u8; SEGMENT / 2]);
    // The other rank fills the second half, and the two together are the whole copy.
    nv::share(&mut two[1], &[9u8; SEGMENT / 2], SEGMENT).expect("the other half");
    assert_eq!(&nv::bytes(&shared)[half..], &[9u8; SEGMENT / 2]);
}

#[test]
fn a_participant_that_brings_the_wrong_number_of_bytes_is_refused() {
    let mut cx = launch(1, 2);
    assert!(
        matches!(
            nv::share(&mut cx[0], b"too long", 2),
            Err(Error::Invalid(Invalid::BadShareLength))
        ),
        "a slice of the wrong length is not a segment"
    );
}

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
    // that nothing about the launch is ambient — every fact comes from the environment the entry
    // supplies, so a second launch in the same process cannot inherit the first one's identity.
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
    let bytes = FrameBytes::try_from(8).unwrap();
    let mut fanin = HashMap::new();
    fanin.insert((0u32, 1u32), 1usize);
    assert_eq!(fanin.len(), 1, "the map is the test's, not the backend's");
    assert_eq!(
        nv::reshape(&mut cx[0], &workers, &edges(&[(0, 1)]), bytes, t(1)),
        Ok(())
    );
    assert_eq!(
        nv::reshape(&mut cx[0], &workers, &edges(&[(0, 1)]), bytes, t(1)),
        Ok(()),
        "a repeated load with the same declaration is accepted"
    );
}
