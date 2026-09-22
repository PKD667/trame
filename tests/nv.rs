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

use crate::contract::{Channel, Deployment, Edge, Error, Wait};
use crate::nv::layout::Layout;
use crate::nv::peers::Fabric;
use crate::nv::{self, Environment};

/// The segment every test's launch publishes into. Small: the partition rule is what is under
/// test, not the size.
const SEGMENT: usize = 64;

/// One launch, as a value per participant.
///
/// The segment is leaked because the launch owns it for its whole life and the contexts borrow it,
/// which is exactly the lifetime the device's arena has.
fn launch(size: u32, depth: u32, capacity: u32) -> Vec<nv::Context> {
    let layout = Layout::new(depth, capacity).expect("a valid layout");
    let fabric = Fabric::new(size, layout);
    let segment: &'static mut [u8] = vec![0u8; SEGMENT].leak();
    (0..size)
        .map(|rank| {
            nv::init(
                Environment {
                    rank,
                    size,
                    fabric: fabric.clone(),
                    segment: segment.as_mut_ptr(),
                    segment_bytes: segment.len(),
                    // No leader in this launch, which is what a null region says.
                    leader_region: std::ptr::null_mut(),
                },
                Deployment::SOLIDARY,
                // One device: every rank is in the one cohort, whatever colour the rule returns.
                |_, _| 0,
            )
            .expect("a launch the model can build")
        })
        .collect()
}

#[test]
fn every_participant_has_its_own_identity_and_endpoints() {
    let cx = launch(3, 2, 16);
    // Distinct identity: this is the assertion a shared global cannot satisfy, because the last
    // writer would make all three answer the same rank.
    assert_eq!(cx.iter().map(nv::rank).collect::<Vec<_>>(), vec![0, 1, 2]);
    assert!(cx.iter().all(|c| nv::size(c) == 3));
    // One device, so one sharing domain led by rank 0, and the whole launch in the cohort.
    assert!(cx.iter().all(|c| nv::hosts(c) == [0, 0, 0]));
    assert!(cx.iter().all(|c| nv::cohort(c) == [0, 1, 2]));
    // Distinct endpoints: rank 0 reaching rank 1 is a different link from rank 2 reaching rank 1,
    // and a launch whose endpoints were one shared value could not deliver both.
    let mut a = launch(2, 2, 16);
    nv::send(&mut a[0], 1, Channel::Message(1), b"from zero", Wait::Poll).expect("accepted");
    nv::send(&mut a[1], 0, Channel::Message(2), b"from one", Wait::Poll).expect("accepted");
    let mut buf = [0u8; 16];
    let got = nv::recv(&mut a[0], &mut buf, Wait::Poll)
        .expect("no fault")
        .expect("one frame");
    assert_eq!((got.source, got.tag), (1, 2));
    assert_eq!(&buf[..got.len as usize], b"from one");
    let got = nv::recv(&mut a[1], &mut buf, Wait::Poll)
        .expect("no fault")
        .expect("one frame");
    assert_eq!((got.source, got.tag), (0, 1));
    assert_eq!(&buf[..got.len as usize], b"from zero");
}

#[test]
fn a_frame_arrives_with_its_tag_and_bytes_paired() {
    let mut cx = launch(2, 4, 32);
    nv::send(&mut cx[0], 1, Channel::Message(9), b"payload", Wait::Poll).expect("accepted");
    nv::send(&mut cx[0], 1, Channel::Message(10), b"second", Wait::Poll).expect("accepted");
    let mut buf = [0u8; 32];
    let first = nv::recv(&mut cx[1], &mut buf, Wait::Poll)
        .expect("no fault")
        .expect("a frame");
    assert_eq!((first.source, first.tag, first.len), (0, 9, 7));
    assert_eq!(&buf[..7], b"payload");
    // FIFO per (source, destination, tag) holds across the two different tags on one link.
    let second = nv::recv(&mut cx[1], &mut buf, Wait::Poll)
        .expect("no fault")
        .expect("a frame");
    assert_eq!((second.tag, second.len), (10, 6));
    assert_eq!(&buf[..6], b"second");
    // And then nothing, which is the normal answer and not an error.
    assert_eq!(nv::recv(&mut cx[1], &mut buf, Wait::Poll), Ok(None));
}

#[test]
fn a_short_buffer_is_reported_without_consuming_the_frame() {
    let mut cx = launch(2, 2, 32);
    nv::send(&mut cx[0], 1, Channel::Message(3), b"abcdefgh", Wait::Poll).expect("accepted");
    let mut small = [0xa5u8; 4];
    assert_eq!(
        nv::recv(&mut cx[1], &mut small, Wait::Poll),
        Err(Error::TooSmall { needed: 8 })
    );
    // The refusal changed nothing: not the caller's buffer, and not the link, so the frame is
    // still there for a caller that grew.
    assert_eq!(small, [0xa5; 4]);
    let mut big = [0u8; 32];
    let got = nv::recv(&mut cx[1], &mut big, Wait::Poll)
        .expect("no fault")
        .expect("a frame");
    assert_eq!(got.len, 8);
    assert_eq!(&big[..8], b"abcdefgh");
}

#[test]
fn a_full_lane_is_capacity_under_poll_and_exhaustion_under_wait() {
    let mut cx = launch(2, 2, 8);
    for _ in 0..2 {
        nv::send(&mut cx[0], 1, Channel::Message(1), b"x", Wait::Poll).expect("accepted");
    }
    // Poll reports capacity pressure at once: the lane is full and waiting would not change it.
    assert_eq!(
        nv::send(&mut cx[0], 1, Channel::Message(1), b"x", Wait::Poll),
        Err(Error::Full)
    );
    // Wait spends its whole budget and says so, distinctly from capacity and from a gone peer.
    match nv::send(&mut cx[0], 1, Channel::Message(1), b"x", Wait::Wait) {
        Err(Error::Exhausted { attempts }) => assert!(attempts > 0),
        other => panic!("expected exhaustion, got {other:?}"),
    }
    // An exhausted send accepted nothing: the two earlier frames are intact and the lane holds
    // exactly them.
    let mut buf = [0u8; 8];
    assert_eq!(
        nv::recv(&mut cx[1], &mut buf, Wait::Poll)
            .unwrap()
            .unwrap()
            .len,
        1
    );
    assert_eq!(
        nv::recv(&mut cx[1], &mut buf, Wait::Poll)
            .unwrap()
            .unwrap()
            .len,
        1
    );
    assert_eq!(nv::recv(&mut cx[1], &mut buf, Wait::Poll), Ok(None));
}

#[test]
fn a_frame_larger_than_the_launched_slot_is_refused() {
    let mut cx = launch(2, 2, 8);
    assert_eq!(
        nv::send(&mut cx[0], 1, Channel::Message(1), &[0u8; 9], Wait::Poll),
        Err(Error::TooLarge { limit: 8 })
    );
    // The destination is checked before the size, so an unreachable peer is `Closed` and not a
    // size complaint about a frame that was never going anywhere.
    assert_eq!(
        nv::send(&mut cx[0], 7, Channel::Message(1), b"x", Wait::Poll),
        Err(Error::Closed)
    );
}

#[test]
fn local_release_is_not_claimed_as_cohort_quiescence() {
    // The capability says which guarantee a caller may rely on, and the call discharges the
    // caller's own obligation. A caller needing proof that peers stopped has to supply it.
    assert_ne!(crate::nv::DECLARATIONS.release, crate::RELEASE_COHORT);
    let mut cx = launch(2, 2, 8);
    nv::release(&mut cx[0]).expect("a local discharge cannot fail");
}

// ---------------------------------------------------------------------------------------------
// Geometry

fn edges(pairs: &[(u32, u32)]) -> Vec<Edge> {
    pairs
        .iter()
        .map(|&(source, destination)| Edge {
            source,
            destination,
            affected: 1,
        })
        .collect()
}

#[test]
fn reshape_validates_before_anything_is_sent() {
    let mut cx = launch(3, 4, 16);
    let workers = [0u32, 1, 2];
    assert_eq!(
        nv::reshape(&mut cx[0], &workers, &edges(&[(0, 1)]), 16, 7),
        Ok(())
    );

    // Unsorted workers: a declaration whose order two participants could read differently.
    assert_eq!(
        nv::reshape(&mut cx[0], &[1, 0, 2], &[], 16, 7),
        Err(Error::Invalid { code: 4 })
    );
    // Duplicate edges cannot both own a slot.
    assert_eq!(
        nv::reshape(&mut cx[0], &workers, &edges(&[(0, 1), (0, 1)]), 16, 7),
        Err(Error::Invalid { code: 7 })
    );
    // A worker that is not a participant of this launch.
    assert_eq!(
        nv::reshape(&mut cx[0], &[0, 1, 9], &[], 16, 7),
        Err(Error::Invalid { code: 5 })
    );
    // A subset is legal: the arena is indexed by rank, so lanes among some participants need no
    // mapping, and requiring the workers to be the whole cohort would refuse a load the launch
    // can serve.
    assert_eq!(nv::reshape(&mut cx[0], &[0, 1], &[], 16, 7), Ok(()));
    // A frame wider than the launched slot is refused at declaration, never truncated at send.
    assert_eq!(
        nv::reshape(&mut cx[0], &workers, &[], 17, 7),
        Err(Error::TooLarge { limit: 16 })
    );
    // A pair whose implied depth the arena cannot hold: the launch sized one depth for every
    // pair, so the widest pair's requirement is what has to fit.
    let mut wide = edges(&[]);
    wide.push(Edge {
        source: 0,
        destination: 1,
        affected: 2,
    });
    assert_eq!(
        nv::reshape(&mut cx[0], &workers, &wide, 16, 7),
        Err(Error::Invalid { code: 9 })
    );
}

#[test]
fn lane_traffic_rides_the_tag_the_load_declared() {
    let mut cx = launch(2, 4, 16);
    let workers = [0u32, 1];
    for c in cx.iter_mut() {
        nv::reshape(c, &workers, &edges(&[(0, 1)]), 16, 42).expect("a valid declaration");
    }
    nv::send(&mut cx[0], 1, Channel::Lane, b"lane", Wait::Poll).expect("accepted");
    let mut buf = [0u8; 16];
    let got = nv::recv(&mut cx[1], &mut buf, Wait::Poll).unwrap().unwrap();
    assert_eq!((got.source, got.tag), (0, 42));

    // Before a load there is no lane tag, so lane traffic has no route and says so.
    let mut fresh = launch(2, 4, 16);
    assert_eq!(
        nv::send(&mut fresh[0], 1, Channel::Lane, b"x", Wait::Poll),
        Err(Error::Invalid { code: 1 })
    );
}

// ---------------------------------------------------------------------------------------------
// Lifetime

#[test]
fn the_segment_is_the_union_of_the_slices() {
    let cx = launch(2, 2, 8);
    // One device, one sharing domain, so both ranks are in the same partition — and a device has
    // no pages, so the split is even rather than page-rounded.
    assert_eq!(nv::slice(&cx[0], 0, SEGMENT), (0, SEGMENT / 2));
    assert_eq!(nv::slice(&cx[0], 1, SEGMENT), (SEGMENT / 2, SEGMENT / 2));
    let mut two = launch(2, 2, 8);
    let shared = nv::share(&mut two[0], &[7u8; SEGMENT / 2], SEGMENT).expect("half the segment");
    assert_eq!(crate::nv::bytes(&shared).len(), SEGMENT);
    assert_eq!(
        &crate::nv::bytes(&shared)[..SEGMENT / 2],
        &[7u8; SEGMENT / 2]
    );
    // The other rank fills the second half, and the two together are the whole copy.
    nv::share(&mut two[1], &[9u8; SEGMENT / 2], SEGMENT).expect("the other half");
    assert_eq!(
        &crate::nv::bytes(&shared)[SEGMENT / 2..],
        &[9u8; SEGMENT / 2]
    );
}

#[test]
fn a_participant_that_brings_the_wrong_number_of_bytes_is_refused() {
    let mut cx = launch(1, 2, 8);
    assert!(
        matches!(
            nv::share(&mut cx[0], b"too long", 2),
            Err(Error::Invalid { code: 11 })
        ),
        "a slice of the wrong length is not a segment"
    );
}

// ---------------------------------------------------------------------------------------------
// Clocks, capabilities, and the routes a device does not have

#[test]
fn readings_are_monotonic_within_one_device() {
    let cx = launch(1, 2, 8);
    let first = nv::reading(&cx[0]).expect("a device clock");
    let second = nv::reading(&cx[0]).expect("a device clock");
    assert_eq!(
        first.clock, second.clock,
        "one device is one comparison domain"
    );
    assert!(second.elapsed >= first.elapsed);
}

#[test]
fn the_declarations_say_what_is_missing_as_well_as_what_is_there() {
    let declared = crate::nv::DECLARATIONS;
    // What it answers. Not "what operations it supports": it exports one surface and answers it,
    // so what is left to declare is the machine facts — what its lanes do, what orderings its
    // atomics reach, and whether a participant is resident.
    assert_eq!(declared.lane_reliability, crate::LANE_RELIABLE);
    assert_eq!(declared.release, crate::RELEASE_LOCAL);
    assert!(declared.resident, "a participant is a resident warp");
    // What it refuses is no longer a field to assert `false` on. A family is a set of names and a
    // backend either has them or does not, so the refusal is that the name does not resolve — a
    // compile error rather than a run-time disappointment, which is the stronger form. What is left
    // to declare here is the machine facts, and those are the rest of this test.
    assert_ne!(declared.release, crate::RELEASE_COHORT);
    assert!(declared.waiting_message > 0, "Wait is a budget of attempts");
    assert!(
        declared.waiting_lane > 0,
        "and it bounds the lane route too"
    );
    assert!(declared.atomic_scopes.participant && declared.atomic_scopes.domain);
    assert!(
        !declared.atomic_scopes.system,
        "there is no second node to be visible to"
    );
    assert_eq!(declared.lowering, "warp-split");
    // The derived constants read the table rather than restating it.
    assert_eq!(crate::LOSSY, declared.lane_reliability == crate::LANE_LOSSY);
    assert_eq!(crate::TAG_LIMIT, declared.tag_limit);
}

#[test]
fn the_launch_a_test_builds_is_the_launch_the_device_builds() {
    // Host-model initialization shall match device initialization. What that means here is
    // that nothing about the launch is ambient — every fact comes from the environment the entry
    // supplies, so a second launch in the same process cannot inherit the first one's identity.
    let first = launch(2, 2, 8);
    let second = launch(4, 2, 8);
    assert_eq!(nv::size(&first[0]), 2);
    assert_eq!(nv::size(&second[0]), 4);
    assert_eq!(nv::cohort(&first[0]), [0, 1]);
    assert_eq!(nv::cohort(&second[0]), [0, 1, 2, 3]);
    assert_eq!(nv::rank(&first[1]), 1);
    assert_eq!(nv::rank(&second[1]), 1);
}

#[test]
fn the_plan_is_the_topology_the_contract_asks_for() {
    // `plan` is gone: `reshape` takes the edges directly, ordered, with no hash map to iterate in
    // an order two participants could disagree about. What replaces the old test is that the
    // declaration is validated against the launch rather than compiled into a table.
    let mut cx = launch(2, 4, 8);
    let workers = [0u32, 1];
    let mut fanin = HashMap::new();
    fanin.insert((0u32, 1u32), 1usize);
    assert_eq!(fanin.len(), 1, "the map is the test's, not the backend's");
    assert_eq!(
        nv::reshape(&mut cx[0], &workers, &edges(&[(0, 1)]), 8, 1),
        Ok(())
    );
    assert_eq!(
        nv::reshape(&mut cx[0], &workers, &edges(&[(0, 1)]), 8, 1),
        Ok(()),
        "a repeated load with the same declaration is accepted"
    );
}
