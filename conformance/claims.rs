// The conformance claims: one body, written against trame's public surface and nothing else, that
// every backend's launcher runs. `trame/conformance/main.rs` runs it under mpirun for the MPI
// family and `trame/nv/tests/conformance.rs` runs it on nv's host model, both by inclusion, so the
// backends all answer the same text. `none` carries no frames and answers none of it. Each claim quotes the `backend.md`
// sentence it checks, which is what makes a pass on every backend evidence for freezing that
// sentence.
//
// A participant prints one JSON line when it starts a claim and one with its verdict, so a
// launcher that times out can name the claim that was running. `CLAIMS` is the inventory
// `trame/scripts/conform.sh` holds every backend to: a claim with no verdict is a FAIL.
//
// No wait here sleeps or asks the host to wait. A wait is repeated attempts, and one that makes
// `SPIN` attempts in a row without progress is a FAIL that says so, because a hang is the one
// outcome that would otherwise hide which claim caused it. S1 and the collective barrier have no
// SPIN bound; the launcher's timeout is their detector.

use std::convert::Infallible;
use std::num::{NonZeroU32, NonZeroU64};

use trame::leader::{self, Leader};
use trame::{
    attach, bytes, detach, BackendFault, Channel, Deployment, Edge, Environment, Error,
    Failure, FailureKind, Handle, Invalid, Launch, LOSSY, MAX_FRAME, Participant, Rank, Tag,
    clock, done, hosts, init, rank, recv, release, reshape, send, size,
};

/// Every claim the body reports, in the order a worker reaches them. `M5` is last because it runs
/// in its own launch, where a hang in `done` can be attributed to it and to nothing else. Read by
/// `conform.sh` from this text, not by Rust.
#[allow(dead_code)]
pub const CLAIMS: &[&str] = &[
    "D1", "E1", "K1", "R1", "M2", "M3", "M4", "M1", "S1", "L1", "L2", "L3", "C1", "C2", "C3", "M5",
];

/// Frames each directed pair carries in M4, L2, L3, R1, C1 and C2. Above the depth of every bounded
/// route a local launch has (4 per ring lane and 8 per nv link as launched by the test), so a
/// sender meets `Full` and has to treat it as its own wait.
const K: u32 = 64;

/// Attempts in a row without progress before a wait is a FAIL. At one attempt per microsecond,
/// the slowest local backend, that is about 17 seconds: far longer than a live peer needs, and
/// shorter than the launcher's timeout, so a stuck wait reports itself before it is killed. S1
/// and the collective barrier do not use this bound; the launcher's timeout detects their stalls.
const SPIN: u64 = 1 << 24;

/// Accepted sends M5 allows before calling the backend unbounded: eight times the largest capacity
/// a local backend states, MPI's 128 MiB attached buffer over 65,544-byte frames.
const PRESSURE: u64 = 1 << 14;

/// The frame every route must carry (`backend.md`: at least 65,544 bytes everywhere).
const FLOOR: usize = 65_544;

/// The lane frame L2 and L3 declare: room for the longest frame `len_of` gives.
const LANE_FRAME: usize = 512;

/// S1's segment: one byte longer than the largest frame this backend carries, so its bytes cannot
/// travel as frames.
pub const SHARE: usize = MAX_FRAME + 1;

const S1_HANDLE: Tag = Tag::new(0x5301);
const S1_DONE: Tag = Tag::new(0x5302);

/// The `i`th byte of S1's revision `revision`: distinct per revision, so a swapped pair is caught.
fn s1_bytes(revision: u64) -> Vec<u8> {
    (0..SHARE).map(|i| ((i as u64 * 7 + revision * 13) % 251) as u8).collect()
}

/// The leader's end of a frame's header, where a worker's end carries its rank.
const LEADER: u32 = u32::MAX;

/// Receive room for every frame except M3's.
const ROOM: usize = 4096;

const M2_TAG: u16 = 20;
const M3_TAG: u16 = 30;
const M4_TAG: u16 = 40;
const L2_TAG: u16 = 50;
const L3_TAG: u16 = 51;
const UP_TAG: u16 = 60;
const DOWN_TAG: u16 = 70;
const M5_TAG: u16 = 80;
const C1_TAG: u16 = 90;
const C2_TAG: u16 = 92;

type Verdict = Result<String, String>;

/// Who is speaking, for the JSON lines.
struct Who(String);

impl Who {
    fn line(&self, claim: &str, tail: &str) {
        println!(
            "{{\"claim\":\"{claim}\",\"backend\":\"{}\",\"participant\":\"{}\",{tail}}}",
            trame::ID.name(),
            self.0
        );
    }

    /// Run one claim between its start line and its verdict line. Returns whether it passed.
    fn claim(&self, claim: &str, check: impl FnOnce() -> Verdict) -> bool {
        self.line(claim, "\"event\":\"start\"");
        self.verdict(claim, check())
    }

    fn verdict(&self, claim: &str, verdict: Verdict) -> bool {
        let (word, detail) = match &verdict {
            Ok(detail) => ("pass", detail),
            Err(detail) => ("fail", detail),
        };
        let detail = detail
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\n', "\\n");
        self.line(claim, &format!("\"verdict\":\"{word}\",\"detail\":\"{detail}\""));
        verdict.is_ok()
    }
}

/// What a claim over pairs exercises when there is one worker: only its own pair.
fn solo(n: u32) -> &'static str {
    if n == 1 { "; one worker, so only its own pair" } else { "" }
}

fn ensure(holds: bool, why: impl FnOnce() -> String) -> Result<(), String> {
    if holds { Ok(()) } else { Err(why()) }
}

fn r(index: u32) -> Rank {
    Rank::from_index(index)
}

/// A frame's length by sequence number: varied, and never longer than `LANE_FRAME`.
fn len_of(seq: u32) -> usize {
    12 + (seq as usize * 37) % 256
}

/// The bytes of frame `seq` from `src` to `dst`: a header naming all three, then a pattern of all
/// three, so a frame delivered to the wrong place, out of order or corrupted cannot match.
fn pattern(src: u32, dst: u32, seq: u32, len: usize) -> Vec<u8> {
    let mut frame: Vec<u8> = [src, dst, seq].iter().flat_map(|w| w.to_le_bytes()).collect();
    frame.extend((12..len).map(|i| (i as u32 ^ src.wrapping_mul(131) ^ dst.wrapping_mul(29) ^ seq.wrapping_mul(7)) as u8));
    frame.truncate(len);
    frame
}

/// The sequence number a frame's header states.
fn seq_of(frame: &[u8]) -> Option<u32> {
    Some(u32::from_le_bytes(frame.get(8..12)?.try_into().ok()?))
}

/// `backend.md`, Coordination boundaries: a worker-only coordination epoch. It is a collective
/// over the entered workers, not a frame, so it does not enter the unfiltered claim receive route
/// and cannot take a frame belonging to another claim.
fn barrier(cx: &mut trame::Context) -> Result<(), String> {
    trame::barrier(cx);
    Ok(())
}

/// Send `K` sequenced frames to each of `outs` and take `K` from each of `ins`, one attempt of
/// each per round, so `Full`, `Busy` and an empty receive are this caller's wait. A received frame
/// must come from one of `ins`, carry `tag_of(seq)`, be `len_of(seq)` long and match `pattern`.
/// Unless `lossy`, `seq` must be the next one from its source: FIFO and no loss. When `lossy`, it
/// must only exceed the last one seen, and the source is finished at its last frame.
#[allow(clippy::too_many_arguments)]
fn exchange<S>(
    s: &mut S,
    me: u32,
    outs: &[u32],
    ins: &[u32],
    lossy: bool,
    tag_of: impl Fn(u32) -> Tag,
    mut send: impl FnMut(&mut S, u32, u32, &[u8]) -> Result<(), Error>,
    mut recv: impl FnMut(&mut S, &mut [u8]) -> Result<Option<(u32, Tag, usize)>, Error>,
) -> Verdict {
    let mut sent = vec![0u32; outs.len()];
    // Per source: frames taken, and the next sequence number expected.
    let mut taken = vec![0u32; ins.len()];
    let mut next = vec![0u32; ins.len()];
    let mut buf = vec![0u8; ROOM];
    let mut turn = 0;
    let mut idle = 0u64;
    let finished = |next: &[u32]| next.iter().all(|&n| n == K);
    while sent.iter().any(|&n| n < K) || !finished(&next) {
        let mut progress = false;
        if let Some(at) = (0..outs.len()).map(|i| (turn + i) % outs.len()).find(|&i| sent[i] < K) {
            let seq = sent[at];
            match send(s, outs[at], seq, &pattern(me, outs[at], seq, len_of(seq))) {
                Ok(()) => {
                    sent[at] += 1;
                    progress = true;
                }
                // The next round tries the next destination, so one full pair does not hold up
                // the others.
                Err(Error::Full | Error::Busy) => {}
                Err(e) => return Err(format!("send of frame {seq} to {}: {e}", outs[at])),
            }
            turn = at + 1;
        }
        match recv(s, &mut buf) {
            Ok(Some((src, tag, len))) => {
                progress = true;
                let at = ins
                    .iter()
                    .position(|&i| i == src)
                    .ok_or(format!("a frame from {src}, which sends nothing here"))?;
                let data = &buf[..len];
                let seq = if lossy {
                    let seq = seq_of(data).ok_or(format!("a {len}-byte frame from {src}"))?;
                    ensure(seq >= next[at] && seq < K, || {
                        format!("from {src}: frame {seq} after {}", next[at])
                    })?;
                    seq
                } else {
                    next[at]
                };
                ensure(seq < K, || format!("from {src}: a frame after the last"))?;
                ensure(tag == tag_of(seq), || {
                    format!("from {src}, frame {seq}: tag {} for {}", tag.get(), tag_of(seq).get())
                })?;
                ensure(data == pattern(src, me, seq, len_of(seq)), || {
                    format!("from {src}: expected frame {seq} ({} bytes), got {len} bytes stating frame {:?}", len_of(seq), seq_of(data))
                })?;
                taken[at] += 1;
                next[at] = seq + 1;
            }
            Ok(None) | Err(Error::Busy) => {}
            Err(e) => return Err(format!("receive: {e}")),
        }
        idle = if progress { 0 } else { idle + 1 };
        ensure(idle < SPIN, || {
            format!("{SPIN} attempts without progress: sent {sent:?} to {outs:?}, took {taken:?} from {ins:?}")
        })?;
    }
    let skipped: u32 = taken.iter().map(|&t| K - t).sum();
    Ok(format!(
        "{K} frames on each of {} outgoing and {} incoming pairs; {skipped} skipped",
        outs.len(),
        ins.len()
    ))
}

/// Take one frame, repeating while none is pending, into `out`.
/// A received frame as an exchange reads it: the sender, or `LEADER` for the rankless leader.
fn seen(frame: trame::Frame) -> (u32, Tag, usize) {
    (frame.source().map_or(LEADER, Rank::get), frame.tag(), frame.len())
}

fn next_frame(cx: &mut trame::Context, out: &mut [u8]) -> Result<trame::Frame, String> {
    for _ in 0..SPIN {
        match recv(cx, out) {
            Ok(Some(frame)) => return Ok(frame),
            Ok(None) | Err(Error::Busy) => {}
            Err(e) => return Err(format!("receive: {e}")),
        }
    }
    Err(format!("no frame after {SPIN} attempts"))
}

/// Send one frame, repeating while it is refused for capacity.
fn put(cx: &mut trame::Context, to: Rank, channel: Channel, data: &[u8]) -> Result<(), String> {
    for _ in 0..SPIN {
        match send(cx, to, channel, data) {
            Ok(()) => return Ok(()),
            Err(Error::Full | Error::Busy) => {}
            Err(e) => return Err(format!("send to {}: {e}", to.get())),
        }
    }
    Err(format!("still refused after {SPIN} attempts"))
}

/// `backend.md`, Identities: "It refuses by name an empty worker list, a duplicate worker, lists
/// of unequal length, and a launch that is both worker and leader."
fn d1() -> Verdict {
    let l = Launch::new;
    let (twice, pair, one, overlap, shared) = ([l(0), l(0)], [l(0), l(1)], [l(2)], [l(1), l(2)], [l(2), l(2)]);
    let cases: [(&str, Result<Deployment<'_>, Invalid>, Invalid); 4] = [
        ("empty", Deployment::new(&[], None), Invalid::EmptyDeployment),
        ("duplicate", Deployment::new(&twice, None), Invalid::DuplicateWorker),
        ("unequal", Deployment::new(&pair, Some(&one)), Invalid::UnequalLists),
        ("worker and leader", Deployment::new(&pair, Some(&overlap)), Invalid::WorkerIsLeader),
    ];
    for (name, got, want) in cases {
        ensure(got == Err(want), || format!("{name}: {got:?}, not {want:?}"))?;
    }
    ensure(Deployment::new(&pair, Some(&shared)).is_ok(), || {
        "a valid deployment was refused".into()
    })?;
    Ok("four refusals by name; a valid deployment accepted".into())
}

/// `backend.md`, Entry: "`rank`, `size` and `hosts` describe workers only" and "`hosts(cx)[r]` is
/// the lowest rank that shares memory with rank `r`."
fn e1(cx: &trame::Context, workers: usize) -> Verdict {
    let (me, n, hosts) = (rank(cx), size(cx), hosts(cx));
    ensure(me.get() < n, || format!("rank {} of size {n}", me.get()))?;
    ensure(n as usize == workers, || format!("size {n} for {workers} workers"))?;
    ensure(hosts.len() == n as usize, || format!("{} host entries for size {n}", hosts.len()))?;
    for q in 0..n {
        let h = hosts[q as usize];
        ensure(h.get() <= q, || format!("hosts[{q}] = {}", h.get()))?;
        ensure(hosts.get(h.get() as usize) == Some(&h), || format!("hosts[{q}] = {} is not its own entry", h.get()))?;
        let lowest = (0..n).find(|&p| hosts[p as usize] == h);
        ensure(lowest == Some(h.get()), || format!("hosts[{q}] = {} but its domain starts at {lowest:?}", h.get()))?;
    }
    Ok(format!("rank {} of {n}; hosts {:?}", me.get(), hosts.iter().map(|h| h.get()).collect::<Vec<_>>()))
}

/// `backend.md`, Clocks: "`Reading::since` subtracts two readings of one clock and refuses any
/// other pair with `ClockMismatch`." Only the first half: no public item yields a second
/// `ClockId`, so the refusal cannot be reached from here.
fn k1() -> Verdict {
    let first = clock::reading();
    let second = clock::reading();
    ensure(first.clock() == second.clock(), || "two readings of one process differ in clock".into())?;
    ensure(second.elapsed() >= first.elapsed(), || {
        format!("went back: {} then {} ns", first.elapsed().nanos(), second.elapsed().nanos())
    })?;
    let since = second.since(first).map_err(|e| format!("since refused one clock: {e:?}"))?;
    Ok(format!("since = {} ns; ClockMismatch not exercised: no public way to obtain a second ClockId", since.nanos()))
}

/// `backend.md`, Routes: "`TooSmall { needed }` leaves the frame in place for a receive with
/// `needed` bytes of room."
fn m2(cx: &mut trame::Context) -> Verdict {
    let (me, n) = (rank(cx).get(), size(cx));
    let (to, from) = ((me + 1) % n, (me + n - 1) % n);
    let frame = pattern(me, to, 0, 100);
    put(cx, r(to), Channel::Message(Tag::new(M2_TAG)), &frame)?;
    let mut small = [0u8; 10];
    let mut refused = None;
    for _ in 0..SPIN {
        match recv(cx, &mut small) {
            Ok(None) | Err(Error::Busy) => {}
            other => {
                refused = Some(other);
                break;
            }
        }
    }
    let needed = match refused {
        Some(Err(Error::TooSmall { needed })) => needed,
        other => return Err(format!("a 10-byte receive of a 100-byte frame: {other:?}")),
    };
    ensure(needed == 100, || format!("needed {needed} for a 100-byte frame"))?;
    let mut exact = vec![0u8; needed];
    let got = recv(cx, &mut exact).map_err(|e| format!("second receive: {e}"))?;
    let got = got.ok_or("the frame was gone after TooSmall")?;
    ensure(got.source() == Some(r(from)) && got.tag() == Tag::new(M2_TAG) && got.len() == 100, || {
        format!("second receive: {got:?}, from {from}")
    })?;
    ensure(exact == pattern(from, me, 0, 100), || "second receive: other bytes".into())?;
    Ok(format!("from {from}: TooSmall {{ needed: 100 }}, then the same frame{}", solo(n)))
}

/// `backend.md`: "`MAX_FRAME: usize` bounds a frame on every route of the build: at least 65,544
/// bytes everywhere."
fn m3(cx: &mut trame::Context) -> Verdict {
    let (me, n) = (rank(cx).get(), size(cx));
    let (to, from) = ((me + 1) % n, (me + n - 1) % n);
    let tag = Channel::Message(Tag::new(M3_TAG));
    let over = send(cx, r(to), tag, &vec![0u8; MAX_FRAME + 1]);
    ensure(over == Err(Error::TooLarge { limit: MAX_FRAME }), || {
        format!("a {}-byte send: {over:?}", MAX_FRAME + 1)
    })?;
    put(cx, r(to), tag, &pattern(me, to, 0, FLOOR))?;
    let mut buf = vec![0u8; FLOOR];
    let got = next_frame(cx, &mut buf)?;
    ensure(got.source() == Some(r(from)) && got.tag() == Tag::new(M3_TAG) && got.len() == FLOOR, || {
        format!("{got:?}, from {from}")
    })?;
    ensure(buf == pattern(from, me, 0, FLOOR), || "the 65,544-byte frame changed in transit".into())?;
    Ok(format!("TooLarge {{ limit: {MAX_FRAME} }}; a {FLOOR}-byte frame from {from} bit-exact{}", solo(n)))
}

/// `backend.md`, Routes: "The `Message` channel is one FIFO per pair across all tags and never
/// loses a frame." Every ordered pair, the worker's own included, under three tags.
fn m4(cx: &mut trame::Context) -> Verdict {
    let (me, n) = (rank(cx).get(), size(cx));
    let all: Vec<u32> = (0..n).collect();
    let tag_of = |seq: u32| Tag::new(M4_TAG + (seq % 3) as u16);
    exchange(
        cx,
        me,
        &all,
        &all,
        false,
        tag_of,
        |cx, to, seq, data| send(cx, r(to), Channel::Message(tag_of(seq)), data),
        |cx, out| Ok(recv(cx, out)?.map(seen)),
    )
    .map(|detail| format!("{detail}{}", solo(n)))
}

/// `backend.md`, Routes: "A refused attempt answers ... for a receive, `Ok(None)`." Checked after
/// M4, between two barriers, when every frame sent has been taken.
fn m1(cx: &mut trame::Context) -> Verdict {
    barrier(cx)?;
    let mut buf = vec![0u8; ROOM];
    let got = recv(cx, &mut buf);
    barrier(cx)?;
    match got {
        Ok(None) => Ok("Ok(None) with nothing pending".into()),
        other => Err(format!("with nothing pending: {other:?}")),
    }
}

/// The revision a handle's bytes state, in the little-endian layout `Handle::to_bytes` writes.
/// `Handle::revision` is crate-private and this body is also the launcher example's, so the
/// documented encoding is read directly here.
fn revision_of(bytes: &[u8; Handle::BYTES]) -> u64 {
    u64::from_le_bytes(bytes[..8].try_into().expect("eight revision bytes"))
}

/// A protocol failure after a handle has gone out: a worker may still be attached, so retiring
/// would free a segment it reads. `eprintln!` the exact failure and abort.
fn abort_s1(why: &str) -> ! {
    eprintln!("S1: {why}");
    std::process::abort()
}

/// Send one handle on the leader route, waiting while it is full. `sent` is whether a handle has
/// already gone out: after that, an unexpected failure aborts rather than retires a segment a
/// worker could still read.
fn send_handle(route: &Leader, to: u32, data: &[u8; Handle::BYTES], sent: bool) -> Result<(), String> {
    loop {
        match route.send(r(to), S1_HANDLE, data) {
            Ok(()) => return Ok(()),
            Err(Error::Full | Error::Busy) => {}
            Err(e) if sent => abort_s1(&format!("sending a handle to {to}: {e}")),
            Err(e) => return Err(format!("sending a handle to {to}: {e}")),
        }
    }
}

/// Retire one revision, reporting a refused cleanup with the owner it returned. That owner drops
/// next, whose `Drop` prints the failing syscall and aborts, so the failure is never silent.
fn retire_one(route: &Leader, segment: trame::leader::Published) -> Result<(), String> {
    match leader::retire(route, segment) {
        Ok(()) => Ok(()),
        Err((segment, e)) => {
            let named = leader::handle(&segment);
            Err(format!("{e}; live handle {named:?}"))
        }
    }
}

/// Receive the next S1 handle frame, asserting its tag, length, absent source and revision. Only
/// the two waits that make no progress are retried; anything else is a refusal.
fn receive_s1_handle(cx: &mut trame::Context, expected: u64) -> Result<Handle, String> {
    let mut buf = [0u8; Handle::BYTES];
    loop {
        match leader::recv(cx, &mut buf) {
            Ok(Some(frame)) => {
                ensure(
                    frame.tag() == S1_HANDLE && frame.len() == Handle::BYTES && frame.source().is_none(),
                    || format!("S1 handle: tag {}, {} bytes, source {:?}", frame.tag().get(), frame.len(), frame.source()),
                )?;
                let revision = revision_of(&buf);
                ensure(revision == expected, || format!("S1 handle revision {revision}, not {expected}"))?;
                return Handle::from_bytes(buf).map_err(|e| format!("S1 handle: {e:?}"));
            }
            Ok(None) | Err(Error::Busy) => {}
            Err(e) => return Err(format!("S1 receiving a handle: {e}")),
        }
    }
}

/// S1 is `backend.md`, Shared memory: the leader publishes two revisions, each longer than one
/// frame; every worker it leads attaches both, reads them bit-identical while both are live,
/// detaches, and says so; the leader retires both only after every one has.
fn s1(cx: &mut trame::Context, led: bool) -> Verdict {
    if trame::ID == trame::Backend::Nv {
        // nv has no device segment. The claim is the refusal itself: attach must refuse with the
        // exact `Unimplemented` failure, and nothing is transferred.
        let mut raw = [0u8; Handle::BYTES];
        raw[0] = 1;
        let handle = Handle::from_bytes(raw).map_err(|e| format!("S1 revision-1 handle: {e:?}"))?;
        // SAFETY: no segment is published on nv; attach refuses before mapping anything.
        let got = unsafe { attach(cx, handle) };
        let want = Error::Failed(Failure {
            participant: Participant::Worker(rank(cx)),
            operation: "attach",
            kind: FailureKind::Backend(BackendFault::Unimplemented),
        });
        return match got {
            Err(e) if e == want => {
                Ok("UNIMPLEMENTED: attach refused with BackendFault::Unimplemented".into())
            }
            Err(other) => Err(format!("S1 nv attach: {other:?}, not {want:?}")),
            Ok(_) => Err("S1 nv attach: unexpectedly mapped a segment".into()),
        };
    }
    if !led {
        return Err("S1 needs a leader: the segment is leader-owned".into());
    }
    // Two fixed-size handles from the leader, revision 1 first, one attempt per wait.
    let one = receive_s1_handle(cx, 1)?;
    let two = receive_s1_handle(cx, 2)?;
    // SAFETY: the leader retires each revision only after this worker's `S1_DONE`, which it sends
    // after detaching below.
    let a = unsafe { attach(cx, one) }.map_err(|e| format!("S1 attaching revision 1: {e}"))?;
    let b = unsafe { attach(cx, two) }.map_err(|e| format!("S1 attaching revision 2: {e}"))?;
    let whole = bytes(&a) == s1_bytes(1) && bytes(&b) == s1_bytes(2);
    ensure(whole, || "S1: the attached bytes are not the leader's two revisions".into())?;
    detach(cx, a).map_err(|(_, e)| format!("S1 detaching revision 1: {e}"))?;
    detach(cx, b).map_err(|(_, e)| format!("S1 detaching revision 2: {e}"))?;
    loop {
        match leader::send(cx, S1_DONE, &[]) {
            Ok(()) => break,
            Err(Error::Full | Error::Busy) => {}
            Err(e) => return Err(format!("S1 sending S1_DONE: {e}")),
        }
    }
    Ok(format!("{SHARE} bytes, revisions 1 and 2 coexisting"))
}

/// The leader's half of S1: publish two revisions, hand both handles to every worker it leads,
/// then retire both only after every one has detached and answered.
fn s1_leader(route: &Leader, mine: &[u32], me: Launch) -> Verdict {
    if trame::ID == trame::Backend::Nv {
        // nv's leader has no device segment to publish. As on the worker, the refusal is the
        // claim: publish must refuse with the exact `Unimplemented` failure.
        let got = leader::publish(route, NonZeroU64::MIN, &s1_bytes(1));
        let want = Error::Failed(Failure {
            participant: Participant::Leader(me),
            operation: "publish",
            kind: FailureKind::Backend(BackendFault::Unimplemented),
        });
        return match got {
            Err(e) if e == want => {
                Ok("UNIMPLEMENTED: publish refused with BackendFault::Unimplemented".into())
            }
            Err(other) => Err(format!("S1 nv publish: {other:?}, not {want:?}")),
            Ok(_) => Err("S1 nv publish: unexpectedly published a segment".into()),
        };
    }
    let one = match leader::publish(route, NonZeroU64::MIN, &s1_bytes(1)) {
        Ok(segment) => segment,
        Err(e) => return Err(format!("S1 publishing revision 1: {e}")),
    };
    let two = match leader::publish(route, NonZeroU64::new(2).expect("two is nonzero"), &s1_bytes(2)) {
        Ok(segment) => segment,
        Err(e) => {
            // Revision 1 is live. Retire it rather than strand it, and report both outcomes.
            return Err(match retire_one(route, one) {
                Ok(()) => format!("S1 publishing revision 2: {e}"),
                Err(r) => format!("S1 publishing revision 2: {e}; retiring revision 1: {r}"),
            });
        }
    };
    let one_handle = leader::handle(&one).to_bytes();
    let two_handle = leader::handle(&two).to_bytes();
    let mut sent = false;
    let mut send_error = None;
    'sends: for &worker in mine {
        for handle in [one_handle, two_handle] {
            match send_handle(route, worker, &handle, sent) {
                Ok(()) => sent = true,
                Err(e) => {
                    send_error = Some(e);
                    break 'sends;
                }
            }
        }
    }
    if let Some(e) = send_error {
        // No handle has gone out (`send_handle` aborts once one has), so no worker can hold either
        // revision: retire both and report.
        return Err(match retire_one(route, one).and_then(|()| retire_one(route, two)) {
            Ok(()) => e,
            Err(r) => format!("{e}; {r}"),
        });
    }
    // Every handle is out. One `S1_DONE` per worker, after it has detached both revisions.
    let mut done: Vec<u32> = Vec::with_capacity(mine.len());
    let mut buf = [0u8; 16];
    while done.len() < mine.len() {
        match route.recv(&mut buf) {
            Ok(Some(frame)) => {
                let source = match frame.source() {
                    Some(source) => source.get(),
                    None => abort_s1("a S1_DONE with no source"),
                };
                if frame.tag() != S1_DONE || frame.len() != 0 || !mine.contains(&source) || done.contains(&source) {
                    abort_s1(&format!(
                        "a S1_DONE with tag {}, {} bytes, from {source}",
                        frame.tag().get(),
                        frame.len()
                    ));
                }
                done.push(source);
            }
            Ok(None) | Err(Error::Busy) => {}
            Err(e) => abort_s1(&format!("receiving a S1_DONE: {e}")),
        }
    }
    retire_one(route, one).map_err(|e| format!("S1 retiring revision 1: {e}"))?;
    retire_one(route, two).map_err(|e| format!("S1 retiring revision 2: {e}"))?;
    Ok(format!("{SHARE} bytes, revisions 1 and 2 coexisting"))
}

/// `backend.md`, Routes: "Each direction is FIFO per worker. A worker the deployment gives no
/// leader gets `Invalid(NoLeader)`." The worker's half.
fn r1_worker(cx: &mut trame::Context, led: bool) -> Verdict {
    if !led {
        let mut buf = [0u8; 8];
        let up = leader::send(cx, Tag::new(UP_TAG), b"x");
        let down = leader::recv(cx, &mut buf);
        let no = Error::Invalid(Invalid::NoLeader);
        ensure(up == Err(no) && down == Err(no), || format!("without a leader: send {up:?}, recv {down:?}"))?;
        return Ok("no leader: send and recv refused with Invalid(NoLeader)".into());
    }
    let me = rank(cx).get();
    exchange(
        cx,
        me,
        &[LEADER],
        &[LEADER],
        false,
        |seq| Tag::new(DOWN_TAG + (seq % 3) as u16),
        |cx, _, seq, data| leader::send(cx, Tag::new(UP_TAG + (seq % 3) as u16), data),
        |cx, out| Ok(leader::recv(cx, out)?.map(seen)),
    )
}

/// The leader's half of R1: `Frame::source` must name the worker that sent it.
fn r1_leader(route: &Leader, mine: &[u32]) -> Verdict {
    exchange(
        &mut (),
        LEADER,
        mine,
        mine,
        false,
        |seq| Tag::new(UP_TAG + (seq % 3) as u16),
        |_, to, seq, data| route.send(r(to), Tag::new(DOWN_TAG + (seq % 3) as u16), data),
        |_, out| Ok(route.recv(out)?.map(seen)),
    )
}

/// `backend.md`, Collectives: "`reshape` declares one load's lanes: ascending unique workers,
/// edges ascending by `(source, destination)` with both ends among the workers, lane frames of at
/// most `frame` bytes." Each violation must be refused by its name.
fn l1(cx: &mut trame::Context) -> Verdict {
    let n = size(cx);
    let all: Vec<Rank> = (0..n).map(r).collect();
    let one = NonZeroU32::MIN;
    let edge = |s, d| Edge::new(r(s), r(d), one);
    let tag = Tag::new(L2_TAG);
    let unordered = if n > 1 { vec![r(1), r(0)] } else { vec![r(0), r(0)] };
    let mut outside = all.clone();
    outside.push(r(n));
    let crossed = if n > 1 { vec![edge(1, 0), edge(0, 1)] } else { vec![edge(0, 0), edge(0, 0)] };
    let cases: [(&str, Result<(), Error>, Error); 5] = [
        ("unordered workers", reshape(cx, &unordered, &[], LANE_FRAME, tag), Error::Invalid(Invalid::UnorderedWorkers)),
        ("a worker outside the job", reshape(cx, &outside, &[], LANE_FRAME, tag), Error::Invalid(Invalid::RankOutsideJob)),
        ("unordered edges", reshape(cx, &all, &crossed, LANE_FRAME, tag), Error::Invalid(Invalid::UnorderedEdges)),
        ("an edge outside the workers", reshape(cx, &[r(0)], &[edge(0, 1)], LANE_FRAME, tag), Error::Invalid(Invalid::EdgeOutsideWorkers)),
        ("a frame over MAX_FRAME", reshape(cx, &all, &[], MAX_FRAME + 1, tag), Error::TooLarge { limit: MAX_FRAME }),
    ];
    for (name, got, want) in cases {
        ensure(got == Err(want), || format!("{name}: {got:?}, not {want:?}"))?;
    }
    Ok(if n > 1 {
        "five refusals by name".into()
    } else {
        "five refusals by name; one worker, so the unordered cases are duplicates".into()
    })
}

/// `backend.md`, Routes: "A `Lane` is FIFO per pair, and when `LOSSY` is true it may skip frames
/// but never reorders them" and "Lane frames carry the tag `reshape` declared." One load over
/// `pairs`, `K` frames per edge.
fn lanes(cx: &mut trame::Context, pairs: &[(u32, u32)], tag: Tag) -> Verdict {
    let (me, n) = (rank(cx).get(), size(cx));
    let all: Vec<Rank> = (0..n).map(r).collect();
    let mut edges: Vec<Edge> = pairs.iter().map(|&(s, d)| Edge::new(r(s), r(d), NonZeroU32::MIN)).collect();
    edges.sort();
    reshape(cx, &all, &edges, LANE_FRAME, tag).map_err(|e| format!("reshape: {e}"))?;
    let outs: Vec<u32> = pairs.iter().filter(|p| p.0 == me).map(|p| p.1).collect();
    let ins: Vec<u32> = pairs.iter().filter(|p| p.1 == me).map(|p| p.0).collect();
    let verdict = exchange(
        cx,
        me,
        &outs,
        &ins,
        LOSSY,
        |_| tag,
        |cx, to, _, data| send(cx, r(to), Channel::Lane, data),
        |cx, out| Ok(recv(cx, out)?.map(seen)),
    );
    barrier(cx)?;
    verdict.map(|detail| format!("{} edges; {detail}", edges.len()))
}

/// L2 over a ring: each worker to the next.
fn l2(cx: &mut trame::Context) -> Verdict {
    let n = size(cx);
    let ring: Vec<(u32, u32)> = (0..n).map(|q| (q, (q + 1) % n)).collect();
    lanes(cx, &ring, Tag::new(L2_TAG))
}

/// `backend.md`: "`release` ends your use of the lanes", after which a second load with other
/// edges holds to L2's claim. All-to-all without self edges; one worker has only its own.
fn l3(cx: &mut trame::Context) -> Verdict {
    release(cx).map_err(|e| format!("release: {e}"))?;
    let n = size(cx);
    let pairs: Vec<(u32, u32)> = if n > 1 {
        (0..n).flat_map(|s| (0..n).filter(move |&d| d != s).map(move |d| (s, d))).collect()
    } else {
        vec![(0, 0)]
    };
    let verdict = lanes(cx, &pairs, Tag::new(L3_TAG));
    release(cx).map_err(|e| format!("second release: {e}"))?;
    verdict.map(|detail| if n > 1 { detail } else { format!("{detail}; one worker, so the edge set is L2's") })
}

/// Enter as a worker. `init` is a collective, so the claim the launch reaches first is announced
/// before it: a process that never leaves `init` then times out inside a named claim.
/// Before `init` a worker has no rank, so it is named by process and thread, which tell apart the
/// participants of one launch whether they are processes or threads.
fn entered(env: Environment, workers: &[Launch], leaders: Option<&[Launch]>, first: &str) -> (Who, Result<trame::Context, Failure>) {
    let who = Who(format!("worker entering, pid {} {:?}", std::process::id(), std::thread::current().id()));
    who.line(first, "\"event\":\"start\"");
    (who, init(env, Deployment::new(workers, leaders).expect("the launcher states a valid deployment")))
}

/// One arm's wait: a step with no progress counts, one with progress resets it, and `SPIN` in a
/// row is a FAIL that names the arm.
fn idle(stalls: &mut u64, arm: &str) -> Result<trame::Step, String> {
    *stalls += 1;
    if *stalls >= SPIN {
        return Err(format!("{arm}: no progress after {SPIN} steps"));
    }
    Ok(trame::Step::Idle)
}

/// Send `seq..K` under `tag` to `to`, the frame being its sequence, keeping the place across
/// refusals.
fn post(io: &mut trame::Io<'_>, to: Rank, tag: Tag, seq: &mut u32, stalls: &mut u64) -> Result<trame::Step, String> {
    while *seq < K {
        match io.send(to, Channel::Message(tag), &seq.to_le_bytes()) {
            Ok(()) => {
                *seq += 1;
                *stalls = 0;
            }
            Err(Error::Full | Error::Busy) => return idle(stalls, "sender"),
            Err(e) => return Err(format!("send: {e}")),
        }
    }
    Ok(trame::Step::Done)
}

/// Take every frame this arm owns into `got`, as source, tag and sequence, until it holds `want`.
/// The buffer is smaller than `MAX_FRAME`, so a backend must size a frame before it takes one.
fn gather(
    io: &mut trame::Io<'_>,
    got: &mut Vec<(Option<Rank>, Tag, u32)>,
    want: usize,
    stalls: &mut u64,
) -> Result<trame::Step, String> {
    let mut buf = [0u8; 16];
    let before = got.len();
    loop {
        match io.recv(&mut buf) {
            Ok(Some(frame)) => {
                let seq = buf.get(..4).filter(|_| frame.len() == 4).ok_or(format!("a {}-byte frame", frame.len()))?;
                got.push((frame.source(), frame.tag(), u32::from_le_bytes(seq.try_into().expect("four bytes"))));
            }
            Ok(None) | Err(Error::Busy) => break,
            Err(e) => return Err(format!("recv: {e}")),
        }
    }
    if got.len() >= want {
        return Ok(trame::Step::Done);
    }
    if got.len() > before {
        *stalls = 0;
        return Ok(trame::Step::Progress);
    }
    idle(stalls, "receiver")
}

/// `backend.md`, Execution: "`concurrent!` given a context lends each arm an `Io` for one step"
/// and "frames from one sending arm to one receiving arm keep their order". Two arms send `K`
/// frames each to the next worker while a third takes the previous worker's.
fn c1(cx: &mut trame::Context) -> Verdict {
    let (me, n) = (rank(cx).get(), size(cx));
    let (to, from) = (r((me + 1) % n), r((me + n - 1) % n));
    let (a, b) = (Tag::new(C1_TAG), Tag::new(C1_TAG + 1));
    let (mut sa, mut sb, mut got) = (0, 0, Vec::new());
    let (mut wa, mut wb, mut wr) = (0, 0, 0);
    trame::concurrent!(cx;
        recv(..) => |io| gather(io, &mut got, 2 * K as usize, &mut wr),
        |io| post(io, to, a, &mut sa, &mut wa),
        |io| post(io, to, b, &mut sb, &mut wb),
    )?;
    let sent: Vec<u32> = (0..K).collect();
    for tag in [a, b] {
        let stream: Vec<u32> = got.iter().filter(|g| g.1 == tag).map(|g| g.2).collect();
        ensure(stream == sent, || format!("tag {}: {stream:?}", tag.get()))?;
    }
    ensure(got.iter().all(|g| g.0 == Some(from)), || format!("a frame not from worker {}", from.get()))?;
    Ok(format!("two arms, {K} frames each to worker {}, each in order{}", to.get(), solo(n)))
}

/// `backend.md`, Execution: "A frame goes to the first arm, in source order, whose `recv` setting
/// names its tag." One arm names a tag, the next receives every other one, and a third sends
/// both, interleaved, so each receiving arm meets frames the other owns.
fn c2(cx: &mut trame::Context) -> Verdict {
    let (me, n) = (rank(cx).get(), size(cx));
    let (to, from) = (r((me + 1) % n), r((me + n - 1) % n));
    let (a, b) = (Tag::new(C2_TAG), Tag::new(C2_TAG + 1));
    let (mut named, mut rest, mut sent) = (Vec::new(), Vec::new(), 0u32);
    let (mut wn, mut wr, mut ws) = (0, 0, 0);
    trame::concurrent!(cx;
        recv(a) => |io| gather(io, &mut named, K as usize, &mut wn),
        recv(..) => |io| gather(io, &mut rest, K as usize, &mut wr),
        |io| {
            while sent < 2 * K {
                let tag = if sent % 2 == 0 { a } else { b };
                match io.send(to, Channel::Message(tag), &(sent / 2).to_le_bytes()) {
                    Ok(()) => {
                        sent += 1;
                        ws = 0;
                    }
                    Err(Error::Full | Error::Busy) => return idle(&mut ws, "sender"),
                    Err(e) => return Err(format!("send: {e}")),
                }
            }
            Ok(trame::Step::Done)
        },
    )?;
    let each: Vec<u32> = (0..K).collect();
    let only = |got: &[(Option<Rank>, Tag, u32)], tag: Tag| {
        got.iter().all(|g| g.0 == Some(from) && g.1 == tag) && got.iter().map(|g| g.2).collect::<Vec<_>>() == each
    };
    ensure(only(&named, a), || format!("the arm naming tag {} took {named:?}", a.get()))?;
    ensure(only(&rest, b), || format!("the arm after it took {rest:?}"))?;
    Ok(format!("tag {} to the arm naming it, tag {} to the one after{}", a.get(), b.get(), solo(n)))
}

/// `backend.md`, Execution: "`recv` on an arm with no setting returns `Invalid(NotReceiving)`."
fn c3(cx: &mut trame::Context) -> Verdict {
    let mut answer = None;
    trame::concurrent!(cx;
        |io| {
            answer = Some(io.recv(&mut [0u8; 8]));
            Ok::<_, String>(trame::Step::Done)
        },
    )?;
    ensure(answer == Some(Err(Error::Invalid(Invalid::NotReceiving))), || format!("{answer:?}"))?;
    Ok("an arm with no recv setting: Invalid(NotReceiving)".into())
}

/// A worker of the main launch: every claim but M5, then `done`. Returns whether all passed.
pub fn worker(env: Environment, workers: &[Launch], leaders: Option<&[Launch]>) -> bool {
    let (entering, cx) = entered(env, workers, leaders, "D1");
    let mut cx = match cx {
        Ok(cx) => cx,
        Err(f) => return entering.verdict("D1", Err(format!("init: {f:?}"))),
    };
    entering.verdict("D1", Ok("init returned".into()));
    let who = Who(format!("worker {}", rank(&cx).get()));
    let led = leaders.is_some();
    let mut ok = who.claim("D1", d1);
    ok &= who.claim("E1", || e1(&cx, workers.len()));
    ok &= who.claim("K1", k1);
    // First of the traffic, so the leader, which enters it straight from `Leader::open`, does not
    // count the workers' other claims as its own wait.
    ok &= who.claim("R1", || r1_worker(&mut cx, led));
    for (name, claim) in [("M2", m2 as fn(&mut trame::Context) -> Verdict), ("M3", m3), ("M4", m4)] {
        ok &= who.claim(name, || {
            barrier(&mut cx)?;
            claim(&mut cx)
        });
    }
    ok &= who.claim("M1", || m1(&mut cx));
    ok &= who.claim("S1", || s1(&mut cx, led));
    ok &= who.claim("L1", || l1(&mut cx));
    ok &= who.claim("L2", || {
        barrier(&mut cx)?;
        l2(&mut cx)
    });
    ok &= who.claim("L3", || l3(&mut cx));
    ok &= who.claim("C1", || {
        barrier(&mut cx)?;
        c1(&mut cx)
    });
    ok &= who.claim("C2", || {
        barrier(&mut cx)?;
        c2(&mut cx)
    });
    ok &= who.claim("C3", || c3(&mut cx));
    ok & done::<Infallible>(&mut cx, Ok(())).is_ok()
}

/// A leader of the main launch, `me` by its launch rank: R1's other half.
pub fn leader(env: Environment, me: Launch, workers: &[Launch], leaders: &[Launch]) -> bool {
    let who = Who(format!("leader {}", me.get()));
    let deployment = Deployment::new(workers, Some(leaders)).expect("the launcher states a valid deployment");
    // The route outlives the verdict: on MPI dropping it is collective with the workers' `done`,
    // and a wait there belongs to whatever the workers are still doing, not to R1.
    // Announced before `open`, which is collective with the workers' `init`.
    who.line("R1", "\"event\":\"start\"");
    let route = match Leader::open(env, deployment) {
        Ok(route) => route,
        Err(f) => return who.verdict("R1", Err(format!("open: {f:?}"))),
    };
    let mine: Vec<u32> = (0..workers.len() as u32).filter(|&q| leaders[q as usize] == me).collect();
    let ok = who.verdict("R1", r1_leader(&route, &mine).map(|detail| format!("workers {mine:?}; {detail}")));
    who.line("S1", "\"event\":\"start\"");
    let ok = ok & who.verdict("S1", s1_leader(&route, &mine, me));
    drop(route);
    ok
}

/// The launch with no leader: R1's refusal, then M5.
///
/// M5 is `backend.md`, Routes: "A refused attempt answers `Full` (no capacity)" and Failure:
/// "`done` does not wait for delivery." Worker 0 sends 65,544-byte frames to worker 1, which never
/// receives, until one is refused with `Full`; then every worker calls `done`, and the verdict is
/// printed only once `done` has returned, so a `done` that waits is a timeout inside M5.
pub fn pressure(env: Environment, workers: &[Launch]) -> bool {
    let (entering, cx) = entered(env, workers, None, "R1");
    let mut cx = match cx {
        Ok(cx) => cx,
        Err(f) => return entering.verdict("R1", Err(format!("init: {f:?}"))),
    };
    entering.verdict("R1", Ok("init returned".into()));
    let (me, n) = (rank(&cx).get(), size(&cx));
    let who = Who(format!("worker {me}"));
    let ok = who.claim("R1", || r1_worker(&mut cx, false));
    who.line("M5", "\"event\":\"start\"");
    let to = 1 % n;
    let sent = if me == 0 {
        let frame = pattern(me, to, 0, FLOOR);
        let (mut accepted, mut busy) = (0u64, 0u64);
        let refused = loop {
            match send(&mut cx, r(to), Channel::Message(Tag::new(M5_TAG)), &frame) {
                Ok(()) => {
                    accepted += 1;
                    if accepted == PRESSURE {
                        break Err(format!("no Full after {PRESSURE} accepted sends"));
                    }
                }
                Err(Error::Full) => break Ok(accepted),
                Err(Error::Busy) if busy < SPIN => busy += 1,
                Err(e) => break Err(format!("after {accepted} accepted sends and {busy} Busy: {e}")),
            }
        };
        eprintln!("M5: worker {me}: {refused:?} after {accepted} accepted sends to {to}; entering done");
        Some(refused)
    } else {
        None
    };
    let finished = done::<Infallible>(&mut cx, Ok(()));
    let verdict = match (sent, finished) {
        (_, Err(f)) => Err(format!("done: {f:?}")),
        (Some(Err(why)), Ok(())) => Err(why),
        (Some(Ok(accepted)), Ok(())) => Ok(format!(
            "Full after {accepted} accepted {FLOOR}-byte sends to {to}, which never receives; done returned{}",
            if n == 1 { " (one worker: the peer is the sender itself)" } else { "" }
        )),
        (None, Ok(())) => Ok("done returned".into()),
    };
    ok & who.verdict("M5", verdict)
}
