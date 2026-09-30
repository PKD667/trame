// The conformance claims: one body, written against trame's public surface and nothing else, that
// every backend's launcher runs. `trame/conformance/main.rs` runs it under mpirun for the MPI
// family and `trame/backends/nv/tests/conformance.rs` runs it on nv's host model, both by inclusion, so the
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
    attach, bytes, detach, Addr, BackendFault, Channel, Deployment, Edge, Environment, Error,
    Failure, FailureKind, Handle, Invalid, Launch, LOSSY, MAX_FRAME, Participant, Tag,
    clock, done, init, rank, recv, release, reshape, send, size,
};

/// Every claim the body reports, in the order a worker reaches them. `M5` and `F1` are last because
/// each runs in its own launch: M5's so a hang in `done` is attributed to it and to nothing else,
/// F1's because it needs two hosts. Read by `conform.sh` from this text, not by Rust.
#[allow(dead_code)]
pub const CLAIMS: &[&str] = &[
    "D1", "E1", "K1", "R1", "M2", "M3", "M4", "M1", "A1", "S1", "L1", "L2", "L3", "C1", "C2", "C3",
    "M5", "F1",
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
const A1_TAG: u16 = 94;
/// F1's `Message` tags are `F1_TAG..F1_TAG + 3`, then its lane, `DONE` and `TooSmall` tags.
const F1_TAG: u16 = 100;
const F1_LANE: u16 = 103;
const F1_DONE: u16 = 104;
const F1_SMALL: u16 = 105;

type Verdict = Result<String, String>;

/// Who is speaking, for the JSON lines.
struct Who(String);

impl Who {
    fn line(&self, claim: &str, tail: &str) {
        let backend = if trame::LOSSY { "lossy" } else { trame::ID.name() };
        println!(
            "{{\"claim\":\"{claim}\",\"backend\":\"{backend}\",\"participant\":\"{}\",{tail}}}",
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

/// A worker of this deployment, as a peer route names it.
fn r(index: u32) -> Addr {
    Addr::Local(index)
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
    mut recv: impl FnMut(&mut S, &mut [u8]) -> Result<Option<(u32, Tag, usize)>, String>,
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
            Ok(None) => {}
            Err(e) => return Err(e),
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

/// A received frame as an exchange reads it: the sender's local rank, `LEADER` for the rankless
/// leader. Any other address is the verdict string that names what actually arrived.
fn seen(frame: trame::Frame) -> Result<(u32, Tag, usize), String> {
    match frame.source() {
        None => Ok((LEADER, frame.tag(), frame.len())),
        Some(Addr::Local(rank)) => Ok((rank, frame.tag(), frame.len())),
        Some(remote @ Addr::Remote { .. }) => {
            Err(format!("a frame from another host's worker, {remote:?}"))
        }
    }
}

/// One checked receive for the exchanges. `Ok(None)` is no progress, which is the same answer as
/// `Busy` to a caller that tries again; an actual transport error becomes a verdict string.
fn checked(
    recv: impl FnOnce(&mut [u8]) -> Result<Option<trame::Frame>, Error>,
    out: &mut [u8],
) -> Result<Option<(u32, Tag, usize)>, String> {
    match recv(out) {
        Ok(Some(frame)) => seen(frame).map(Some),
        Ok(None) | Err(Error::Busy) => Ok(None),
        Err(e) => Err(format!("receive: {e}")),
    }
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
fn put(cx: &mut trame::Context, to: Addr, channel: Channel, data: &[u8]) -> Result<(), String> {
    for _ in 0..SPIN {
        match send(cx, to, channel, data) {
            Ok(()) => return Ok(()),
            Err(Error::Full | Error::Busy) => {}
            Err(e) => return Err(format!("send to {to:?}: {e}")),
        }
    }
    Err(format!("still refused after {SPIN} attempts"))
}

/// `backend.md`, Identities: "It rejects an empty table or an empty row, a table too large to
/// number, a `here` outside the table, a `Launch` listed twice, and a leader listed in the table."
fn d1() -> Verdict {
    let l = Launch::new;
    let (a, b, c) = ([l(0), l(1)], [l(2), l(3)], [l(1), l(4)]);
    let none: [Launch; 0] = [];
    let (valid, empty_row, twice): ([&[Launch]; 2], [&[Launch]; 2], [&[Launch]; 2]) =
        ([&a, &b], [&a, &none], [&a, &c]);
    let cases: [(&str, Result<Deployment<'_>, Invalid>, Invalid); 5] = [
        ("empty table", Deployment::new(&[], 0, l(9)), Invalid::EmptyDeployment),
        ("empty row", Deployment::new(&empty_row, 0, l(9)), Invalid::EmptyDeployment),
        ("here outside", Deployment::new(&valid, 2, l(9)), Invalid::RankOutsideJob),
        ("duplicate across hosts", Deployment::new(&twice, 0, l(9)), Invalid::DuplicateWorker),
        ("leader in the table", Deployment::new(&valid, 0, l(3)), Invalid::WorkerIsLeader),
    ];
    for (name, got, want) in cases {
        ensure(got == Err(want), || format!("{name}: {got:?}, not {want:?}"))?;
    }
    ensure(Deployment::new(&valid, 1, l(9)).is_ok(), || {
        "a valid two-host table was refused".into()
    })?;
    Ok("five refusals by name; a valid two-host table accepted".into())
}

/// `backend.md`, Entry: "`rank` and `size` describe this deployment's workers only."
fn e1(cx: &trame::Context, workers: usize) -> Verdict {
    let (me, n) = (rank(cx), size(cx));
    ensure(me < n, || format!("rank {me} of size {n}"))?;
    ensure(n as usize == workers, || format!("size {n} for {workers} workers"))?;
    Ok(format!("rank {me} of {n}"))
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
    let (me, n) = (rank(cx), size(cx));
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
    let (me, n) = (rank(cx), size(cx));
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
    let (me, n) = (rank(cx), size(cx));
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
        |cx, out| checked(|out| recv(cx, out), out),
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

/// `backend.md`, Addresses: "An address that names no worker of the launch, a `Remote` naming your
/// own host included, is `Invalid(RankOutsideJob)`", and Declared lanes: an edge has "at least one
/// endpoint `Local`". A refused send delivers nothing, so after a barrier nothing is pending. That
/// every source is `Local` inside the deployment is checked where frames arrive: M2, M3, C1 and C2
/// compare it to `Some(Local(from))`, and M4's exchange fails on any other. A local-only API cannot
/// be handed a remote worker at all, because it takes a `u32`, so there is no runtime case for it.
/// A worker of another host that is in range has no route on nv yet: its send must fail
/// `Unimplemented`, and the verdict says so the way S1's does. On MPI, F1 checks delivery to
/// in-range remote workers; A1 checks that out-of-range addresses deliver nothing.
fn a1(cx: &mut trame::Context, hosts: &[&[Launch]], here: u16) -> Verdict {
    let n = size(cx);
    let tag = Channel::Message(Tag::new(A1_TAG));
    let count = u16::try_from(hosts.len()).map_err(|_| format!("{} hosts", hosts.len()))?;
    let others: Vec<u16> = (0..count).filter(|&h| h != here).collect();
    let mut outside = vec![
        ("Local(size)".to_string(), Addr::Local(n)),
        ("Remote on this host".to_string(), Addr::Remote { host: here, rank: 0 }),
        ("Remote past the last host".to_string(), Addr::Remote { host: count, rank: 0 }),
    ];
    for &h in &others {
        let past = u32::try_from(hosts[usize::from(h)].len()).map_err(|_| format!("host {h}"))?;
        outside.push((format!("Remote past host {h}'s workers"), Addr::Remote { host: h, rank: past }));
    }
    for (name, to) in &outside {
        let got = send(cx, *to, tag, b"a1");
        ensure(got == Err(Error::Invalid(Invalid::RankOutsideJob)), || format!("{name}: {got:?}"))?;
    }
    let want = Error::Failed(Failure {
        participant: Participant::Worker(rank(cx)),
        operation: "send",
        kind: FailureKind::Backend(BackendFault::Unimplemented),
    });
    if trame::ID == trame::Backend::Nv {
        for &h in &others {
            let got = send(cx, Addr::Remote { host: h, rank: 0 }, tag, b"a1");
            ensure(got == Err(want), || format!("a worker of host {h}: {got:?}, not {want:?}"))?;
        }
    }
    let far = Addr::Remote { host: count, rank: 0 };
    let got = reshape(cx, &[], &[Edge::new(far, far, NonZeroU32::MIN)], LANE_FRAME, Tag::new(A1_TAG));
    ensure(got == Err(Error::Invalid(Invalid::EdgeOutsideWorkers)), || format!("an edge with no Local end: {got:?}"))?;
    barrier(cx)?;
    let mut buf = vec![0u8; ROOM];
    let pending = recv(cx, &mut buf);
    barrier(cx)?;
    ensure(pending == Ok(None), || format!("after only refused sends: {pending:?}"))?;
    let refused = format!("{} refusals by name, nothing delivered", outside.len() + 1);
    Ok(if others.is_empty() {
        format!("{refused}; one host, so no remote worker is in range")
    } else if trame::ID == trame::Backend::Nv {
        format!("UNIMPLEMENTED: send to a remote worker refused with BackendFault::Unimplemented; {refused}")
    } else {
        format!("{refused}; in-range remote delivery checked by F1")
    })
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
        match route.send(to, S1_HANDLE, data) {
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
fn s1(cx: &mut trame::Context) -> Verdict {
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
                    Some(Addr::Local(source)) => source,
                    other => abort_s1(&format!("a S1_DONE from {other:?}, not a worker here")),
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

/// `backend.md`, Leaders: "Each direction is FIFO per worker." The worker's half.
fn r1_worker(cx: &mut trame::Context) -> Verdict {
    let me = rank(cx);
    exchange(
        cx,
        me,
        &[LEADER],
        &[LEADER],
        false,
        |seq| Tag::new(DOWN_TAG + (seq % 3) as u16),
        |cx, _, seq, data| leader::send(cx, Tag::new(UP_TAG + (seq % 3) as u16), data),
        |cx, out| checked(|out| leader::recv(cx, out), out),
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
        |_, to, seq, data| route.send(to, Tag::new(DOWN_TAG + (seq % 3) as u16), data),
        |_, out| checked(|out| route.recv(out), out),
    )
}

/// `backend.md`, Collectives: "`reshape` declares one load's lanes: ascending unique workers,
/// edges ascending by `(source, destination)` with both ends among the workers, lane frames of at
/// most `frame` bytes." Each violation must be refused by its name.
fn l1(cx: &mut trame::Context) -> Verdict {
    let n = size(cx);
    let all: Vec<u32> = (0..n).collect();
    let one = NonZeroU32::MIN;
    let edge = |s, d| Edge::new(r(s), r(d), one);
    let tag = Tag::new(L2_TAG);
    let unordered = if n > 1 { vec![1, 0] } else { vec![0, 0] };
    let mut outside = all.clone();
    outside.push(n);
    let crossed = if n > 1 { vec![edge(1, 0), edge(0, 1)] } else { vec![edge(0, 0), edge(0, 0)] };
    let cases: [(&str, Result<(), Error>, Error); 5] = [
        ("unordered workers", reshape(cx, &unordered, &[], LANE_FRAME, tag), Error::Invalid(Invalid::UnorderedWorkers)),
        ("a worker outside the job", reshape(cx, &outside, &[], LANE_FRAME, tag), Error::Invalid(Invalid::RankOutsideJob)),
        ("unordered edges", reshape(cx, &all, &crossed, LANE_FRAME, tag), Error::Invalid(Invalid::UnorderedEdges)),
        ("an edge outside the workers", reshape(cx, &[0], &[edge(0, 1)], LANE_FRAME, tag), Error::Invalid(Invalid::EdgeOutsideWorkers)),
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
    let (me, n) = (rank(cx), size(cx));
    let all: Vec<u32> = (0..n).collect();
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
        |cx, out| checked(|out| recv(cx, out), out),
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
fn entered(env: Environment, hosts: &[&[Launch]], here: u16, leader: Launch, first: &str) -> (Who, Result<trame::Context, Failure>) {
    let who = Who(format!("worker entering, pid {} {:?}", std::process::id(), std::thread::current().id()));
    who.line(first, "\"event\":\"start\"");
    (who, init(env, Deployment::new(hosts, here, leader).expect("the launcher states a valid deployment")))
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

/// Sends `seq..K` under `tag` to `to`, the frame being its sequence, keeping the place across
/// refusals.
#[trame::process]
struct Post {
    to: Addr,
    tag: Tag,
    seq: u32,
    stalls: u64,
}

impl Post {
    fn step(&mut self, io: &mut trame::Io<'_>) -> Result<trame::Step, String> {
        while self.seq < K {
            match io.send(self.to, Channel::Message(self.tag), &self.seq.to_le_bytes()) {
                Ok(()) => {
                    self.seq += 1;
                    self.stalls = 0;
                }
                Err(Error::Full | Error::Busy) => return idle(&mut self.stalls, "sender"),
                Err(e) => return Err(format!("send: {e}")),
            }
        }
        Ok(trame::Step::Done)
    }
}

/// Takes every frame this arm owns into `got`, as source, tag and sequence, until it holds `want`.
/// The buffer is smaller than `MAX_FRAME`, so a backend must size a frame before it takes one.
#[trame::process]
struct Gather {
    got: Vec<(Option<Addr>, Tag, u32)>,
    want: usize,
    stalls: u64,
}

impl Gather {
    fn new(want: usize) -> Self {
        Gather { got: Vec::new(), want, stalls: 0 }
    }

    fn step(&mut self, io: &mut trame::Io<'_>) -> Result<trame::Step, String> {
        let mut buf = [0u8; 16];
        let before = self.got.len();
        loop {
            match io.recv(&mut buf) {
                Ok(Some(frame)) => {
                    let seq = buf.get(..4).filter(|_| frame.len() == 4).ok_or(format!("a {}-byte frame", frame.len()))?;
                    self.got.push((frame.source(), frame.tag(), u32::from_le_bytes(seq.try_into().expect("four bytes"))));
                }
                Ok(None) | Err(Error::Busy) => break,
                Err(e) => return Err(format!("recv: {e}")),
            }
        }
        if self.got.len() >= self.want {
            return Ok(trame::Step::Done);
        }
        if self.got.len() > before {
            self.stalls = 0;
            return Ok(trame::Step::Progress);
        }
        idle(&mut self.stalls, "receiver")
    }
}

/// Sends `K` frames under each of two tags, alternating, each carrying its place in its own stream.
#[trame::process]
struct Interleave {
    to: Addr,
    tags: [Tag; 2],
    sent: u32,
    stalls: u64,
}

impl Interleave {
    fn step(&mut self, io: &mut trame::Io<'_>) -> Result<trame::Step, String> {
        while self.sent < 2 * K {
            let tag = self.tags[(self.sent % 2) as usize];
            match io.send(self.to, Channel::Message(tag), &(self.sent / 2).to_le_bytes()) {
                Ok(()) => {
                    self.sent += 1;
                    self.stalls = 0;
                }
                Err(Error::Full | Error::Busy) => return idle(&mut self.stalls, "sender"),
                Err(e) => return Err(format!("send: {e}")),
            }
        }
        Ok(trame::Step::Done)
    }
}

/// Asks for a frame from an arm that named no tag.
#[trame::process]
struct Unnamed {
    answer: Option<Result<Option<trame::Frame>, Error>>,
}

impl Unnamed {
    fn step(&mut self, io: &mut trame::Io<'_>) -> Result<trame::Step, String> {
        self.answer = Some(io.recv(&mut [0u8; 8]));
        Ok(trame::Step::Done)
    }
}

/// `backend.md`, Execution: "`concurrent!` given a context lends each arm an `Io` for one step"
/// and "frames from one sending arm to one receiving arm keep their order". Two arms send `K`
/// frames each to the next worker while a third takes the previous worker's.
fn c1(cx: &mut trame::Context) -> Verdict {
    let (me, n) = (rank(cx), size(cx));
    let (to, from) = (r((me + 1) % n), r((me + n - 1) % n));
    let (a, b) = (Tag::new(C1_TAG), Tag::new(C1_TAG + 1));
    let mut taken = Gather::new(2 * K as usize);
    trame::concurrent!(cx;
        recv(..) => &mut taken,
        Post { to, tag: a, seq: 0, stalls: 0 },
        Post { to, tag: b, seq: 0, stalls: 0 },
    )?;
    let got = taken.got;
    let sent: Vec<u32> = (0..K).collect();
    for tag in [a, b] {
        let stream: Vec<u32> = got.iter().filter(|g| g.1 == tag).map(|g| g.2).collect();
        ensure(stream == sent, || format!("tag {}: {stream:?}", tag.get()))?;
    }
    ensure(got.iter().all(|g| g.0 == Some(from)), || format!("a frame not from worker {from:?}"))?;
    Ok(format!("two arms, {K} frames each to worker {to:?}, each in order{}", solo(n)))
}

/// `backend.md`, Execution: "A frame goes to the first arm, in source order, whose `recv` setting
/// names its tag." One arm names a tag, the next receives every other one, and a third sends
/// both, interleaved, so each receiving arm meets frames the other owns.
fn c2(cx: &mut trame::Context) -> Verdict {
    let (me, n) = (rank(cx), size(cx));
    let (to, from) = (r((me + 1) % n), r((me + n - 1) % n));
    let (a, b) = (Tag::new(C2_TAG), Tag::new(C2_TAG + 1));
    let (mut named, mut rest) = (Gather::new(K as usize), Gather::new(K as usize));
    trame::concurrent!(cx;
        recv(a) => &mut named,
        recv(..) => &mut rest,
        Interleave { to, tags: [a, b], sent: 0, stalls: 0 },
    )?;
    let (named, rest) = (named.got, rest.got);
    let each: Vec<u32> = (0..K).collect();
    let only = |got: &[(Option<Addr>, Tag, u32)], tag: Tag| {
        got.iter().all(|g| g.0 == Some(from) && g.1 == tag) && got.iter().map(|g| g.2).collect::<Vec<_>>() == each
    };
    ensure(only(&named, a), || format!("the arm naming tag {} took {named:?}", a.get()))?;
    ensure(only(&rest, b), || format!("the arm after it took {rest:?}"))?;
    Ok(format!("tag {} to the arm naming it, tag {} to the one after{}", a.get(), b.get(), solo(n)))
}

/// `backend.md`, Execution: "`recv` on an arm with no setting returns `Invalid(NotReceiving)`."
fn c3(cx: &mut trame::Context) -> Verdict {
    let mut unnamed = Unnamed { answer: None };
    trame::concurrent!(cx; &mut unnamed)?;
    let answer = unnamed.answer;
    ensure(answer == Some(Err(Error::Invalid(Invalid::NotReceiving))), || format!("{answer:?}"))?;
    Ok("an arm with no recv setting: Invalid(NotReceiving)".into())
}

/// F1's Lane phase: one exchange of lanes with every worker of the other hosts, in one arm.
#[trame::process]
struct Lanes<'a> {
    link: &'a mut Cross,
    me: u32,
    others: &'a [u32],
    lane: Tag,
    report: String,
}

impl Lanes<'_> {
    fn step(&mut self, io: &mut trame::Io<'_>) -> Result<trame::Step, String> {
        let lane = self.lane;
        self.report = exchange(
            &mut (io, &mut *self.link),
            self.me,
            self.others,
            self.others,
            false,
            |_| lane,
            |(io, l), to, _, data| {
                if !l.ready[to as usize] {
                    return Err(Error::Busy);
                }
                io.send(l.addr(to), Channel::Lane, data)
            },
            |(io, l), out| l.take(out, |o| io.recv(o)),
        )?;
        Ok(trame::Step::Done)
    }
}

/// F1's view of the launch: every worker by F1's id (its place in the table read host by host),
/// and which workers of other hosts have sent `DONE`. It holds no endpoint: the Message phases
/// receive through the context, the Lane phase through its arm's `Io`.
struct Cross {
    every: Vec<(u16, u32)>,
    ready: Vec<bool>,
}

impl Cross {
    fn id(&self, host: u16, rank: u32) -> Option<u32> {
        self.every.iter().position(|&w| w == (host, rank)).map(|at| at as u32)
    }

    fn addr(&self, id: u32) -> Addr {
        let (host, rank) = self.every[id as usize];
        Addr::Remote { host, rank }
    }

    /// One receive, as an exchange reads it. A `DONE` from another host is taken here: it marks
    /// its sender ready for lanes and is no frame of the exchange.
    fn take(
        &mut self,
        out: &mut [u8],
        recv: impl FnOnce(&mut [u8]) -> Result<Option<trame::Frame>, Error>,
    ) -> Result<Option<(u32, Tag, usize)>, String> {
        let frame = match recv(out) {
            Ok(Some(frame)) => frame,
            Ok(None) | Err(Error::Busy) => return Ok(None),
            Err(e) => return Err(format!("receive: {e}")),
        };
        let addr = frame.source();
        let source = match addr {
            Some(Addr::Remote { host, rank }) => self.id(host, rank),
            _ => None,
        };
        let id = source.ok_or_else(|| format!("unexpected F1 frame source {addr:?}"))?;
        if frame.tag() == Tag::new(F1_DONE) {
            self.ready[id as usize] = true;
            return Ok(None);
        }
        Ok(Some((id, frame.tag(), frame.len())))
    }
}

/// `backend.md`, Links: "You get the same channel, the same one-attempt outcomes and the same FIFO
/// per directed pair. A link never loses a frame on any backend, even where `LOSSY` lets a local
/// lane skip." Every worker exchanges `K` `Message` frames, then `K` `Lane` frames, with every
/// worker of the other hosts. The receiver must see each sender as `Remote { host, rank }` and its
/// frames in order with none skipped, `LOSSY` or not.
///
/// `barrier` does not cross hosts, so the phases are separated by receive ownership and `DONE`. A
/// worker sends `DONE` to each peer after its `Message` exchange, and sends lanes to a peer only
/// once that peer's `DONE` has arrived, so no lane frame reaches a worker still counting `Message`
/// frames. Nothing stops a peer from finishing its lanes and sending its `TooSmall` frame while
/// this worker is still receiving lanes, so the Lane exchange receives in a `concurrent!` arm
/// owning only the `DONE` and lane tags: the early frame stays at the backend for the unfiltered
/// `TooSmall` receive after `release`. Those two tags belong to different channels, so no order
/// across channels is assumed. Then, on a link, `TooSmall` leaves the frame in place, and `reshape` refuses an edge to a worker past another
/// host's row. One machine proves the protocol, not cross-machine behaviour.
fn f1(cx: &mut trame::Context, hosts: &[&[Launch]], here: u16) -> Verdict {
    let (mine, n) = (rank(cx), size(cx));
    let every: Vec<(u16, u32)> = (0..hosts.len())
        .flat_map(|h| (0..hosts[h].len()).map(move |r| (h as u16, r as u32)))
        .collect();
    let ready = vec![false; every.len()];
    let mut link = Cross { every, ready };
    let me = link.id(here, mine).ok_or(format!("worker {mine} of host {here} is not in the table"))?;
    let others: Vec<u32> = (0..link.every.len() as u32)
        .filter(|&g| link.every[g as usize].0 != here)
        .collect();
    let other = link.every[*others.first().ok_or("one host: no link to exercise")? as usize].0;

    let edge = |s, d| Edge::new(s, d, NonZeroU32::MIN);
    let lane = Tag::new(F1_LANE);
    let locals: Vec<u32> = (0..n).collect();
    let past = Addr::Remote { host: other, rank: hosts[usize::from(other)].len() as u32 };
    let got = reshape(cx, &locals, &[edge(Addr::Local(0), past)], LANE_FRAME, lane);
    ensure(got == Err(Error::Invalid(Invalid::RankOutsideJob)), || format!("an edge to {past:?}: {got:?}"))?;
    let mut edges = Vec::new();
    for &q in &locals {
        for &g in &others {
            edges.push(edge(Addr::Local(q), link.addr(g)));
            edges.push(edge(link.addr(g), Addr::Local(q)));
        }
    }
    edges.sort();
    reshape(cx, &locals, &edges, LANE_FRAME, lane).map_err(|e| format!("reshape: {e}"))?;

    let tag_of = |seq: u32| Tag::new(F1_TAG + (seq % 3) as u16);
    let message = exchange(
        &mut (&mut *cx, &mut link),
        me,
        &others,
        &others,
        false,
        tag_of,
        |(cx, l), to, seq, data| send(cx, l.addr(to), Channel::Message(tag_of(seq)), data),
        |(cx, l), out| l.take(out, |o| recv(cx, o)),
    )
    .map_err(|e| format!("Message: {e}"))?;
    for &g in &others {
        let to = link.addr(g);
        put(cx, to, Channel::Message(Tag::new(F1_DONE)), b"done")?;
    }
    let mut exchanging = Lanes { link: &mut link, me, others: &others, lane, report: String::new() };
    trame::concurrent!(cx; recv(Tag::new(F1_DONE), lane) => &mut exchanging)
        .map_err(|e| format!("Lane: {e}"))?;
    let lanes = exchanging.report;
    release(cx).map_err(|e| format!("release: {e}"))?;

    for &g in &others {
        let to = link.addr(g);
        put(cx, to, Channel::Message(Tag::new(F1_SMALL)), &pattern(me, g, 0, 100))?;
    }
    let mut from = Vec::new();
    for _ in &others {
        let mut small = [0u8; 10];
        let refused = (0..SPIN)
            .map(|_| recv(cx, &mut small))
            .find(|got| !matches!(got, Ok(None) | Err(Error::Busy)));
        ensure(matches!(refused, Some(Err(Error::TooSmall { needed: 100 }))), || {
            format!("a 10-byte receive of a 100-byte link frame: {refused:?}")
        })?;
        let mut exact = [0u8; 100];
        let (src, tag, len) = link
            .take(&mut exact, |o| recv(cx, o))
            .map_err(|e| format!("second receive: {e}"))?
            .ok_or("the frame was gone after TooSmall")?;
        ensure(others.contains(&src) && tag == Tag::new(F1_SMALL) && len == 100, || {
            format!("second receive: from {src}, tag {}, {len} bytes", tag.get())
        })?;
        ensure(exact == pattern(src, me, 0, 100)[..], || format!("from {src}: other bytes"))?;
        from.push(src);
    }
    from.sort();
    ensure(from == others, || format!("TooSmall frames from {from:?}, not {others:?}"))?;
    Ok(format!(
        "Message: {message}; Lane: {lanes}; TooSmall left each of {} link frames in place; an edge to {past:?} refused; one machine: the protocol, not cross-machine behaviour",
        others.len()
    ))
}

/// A worker of the main launch: every claim but M5, then `done`. Returns whether all passed.
pub fn worker(env: Environment, hosts: &[&[Launch]], here: u16, leader: Launch) -> bool {
    let (entering, cx) = entered(env, hosts, here, leader, "D1");
    let mut cx = match cx {
        Ok(cx) => cx,
        Err(f) => return entering.verdict("D1", Err(format!("init: {f:?}"))),
    };
    entering.verdict("D1", Ok("init returned".into()));
    let who = Who(format!("worker {}", rank(&cx)));
    let mut ok = who.claim("D1", d1);
    ok &= who.claim("E1", || e1(&cx, hosts[usize::from(here)].len()));
    ok &= who.claim("K1", k1);
    // First of the traffic, so the leader, which enters it straight from `Leader::open`, does not
    // count the workers' other claims as its own wait.
    ok &= who.claim("R1", || r1_worker(&mut cx));
    for (name, claim) in [("M2", m2 as fn(&mut trame::Context) -> Verdict), ("M3", m3), ("M4", m4)] {
        ok &= who.claim(name, || {
            barrier(&mut cx)?;
            claim(&mut cx)
        });
    }
    ok &= who.claim("M1", || m1(&mut cx));
    ok &= who.claim("A1", || a1(&mut cx, hosts, here));
    ok &= who.claim("S1", || s1(&mut cx));
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

/// The leader of the main launch, `me` by its launch rank: R1's other half.
pub fn leader(env: Environment, me: Launch, hosts: &[&[Launch]], here: u16) -> bool {
    let who = Who(format!("leader {}", me.get()));
    let deployment = Deployment::new(hosts, here, me).expect("the launcher states a valid deployment");
    // The route outlives the verdict: on MPI dropping it is collective with the workers' `done`,
    // and a wait there belongs to whatever the workers are still doing, not to R1.
    // Announced before `open`, which is collective with the workers' `init`.
    who.line("R1", "\"event\":\"start\"");
    let route = match Leader::open(env, deployment) {
        Ok(route) => route,
        Err(f) => return who.verdict("R1", Err(format!("open: {f:?}"))),
    };
    let mine: Vec<u32> = (0..hosts[usize::from(here)].len() as u32).collect();
    let ok = who.verdict("R1", r1_leader(&route, &mine).map(|detail| format!("workers {mine:?}; {detail}")));
    who.line("S1", "\"event\":\"start\"");
    let ok = ok & who.verdict("S1", s1_leader(&route, &mine, me));
    drop(route);
    ok
}

/// A worker of the link launch: F1 alone, then `done`. Two hosts each have a worker 0, so a worker
/// is named by its host too.
pub fn link(env: Environment, hosts: &[&[Launch]], here: u16, leader: Launch) -> bool {
    let (entering, cx) = entered(env, hosts, here, leader, "F1");
    let mut cx = match cx {
        Ok(cx) => cx,
        Err(f) => return entering.verdict("F1", Err(format!("init: {f:?}"))),
    };
    entering.verdict("F1", Ok("init returned".into()));
    let who = Who(format!("host {here} worker {}", rank(&cx)));
    let ok = who.claim("F1", || f1(&mut cx, hosts, here));
    ok & done::<Infallible>(&mut cx, Ok(())).is_ok()
}

/// The pressure launch: M5 alone, with a leader that carries nothing.
///
/// M5 is `backend.md`, Routes: "A refused attempt answers `Full` (no capacity)" and Failure:
/// "`done` does not wait for delivery." Worker 0 sends 65,544-byte frames to worker 1, which never
/// receives, until one is refused with `Full`; then every worker calls `done`, and the verdict is
/// printed only once `done` has returned, so a `done` that waits is a timeout inside M5.
pub fn pressure(env: Environment, hosts: &[&[Launch]], here: u16, leader: Launch) -> bool {
    let (entering, cx) = entered(env, hosts, here, leader, "M5");
    let mut cx = match cx {
        Ok(cx) => cx,
        Err(f) => return entering.verdict("M5", Err(format!("init: {f:?}"))),
    };
    entering.verdict("M5", Ok("init returned".into()));
    let (me, n) = (rank(&cx), size(&cx));
    let who = Who(format!("worker {me}"));
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
    who.verdict("M5", verdict)
}

/// The leader of a launch whose claim needs none, the pressure launch's or a host's in the link
/// launch: it opens the route every deployment has and holds it until the workers' `done`, which
/// on MPI is collective with its drop. It carries no frame and makes no claim; a failure to open
/// is reported under `claim`, the one its workers are in.
pub fn pressure_leader(env: Environment, me: Launch, hosts: &[&[Launch]], here: u16, claim: &str) -> bool {
    let who = Who(format!("leader {}", me.get()));
    let deployment = Deployment::new(hosts, here, me).expect("the launcher states a valid deployment");
    match Leader::open(env, deployment) {
        Ok(route) => {
            drop(route);
            true
        }
        Err(f) => who.verdict(claim, Err(format!("open: {f:?}"))),
    }
}
