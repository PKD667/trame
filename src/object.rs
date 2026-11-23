//! Objects between leaders: INTERFACE.md 6, Definition (Objects).
//!
//! The layers. A backend carries frames: `Backend::peer_send` and `peer_recv` move one frame of `FRAME` bytes
//! between two leaders, in order per sender and per tag, and `Backend::share` moves bytes to the other processes of
//! a host. This file builds objects on that and nothing else: an `Object` is encoded, cut into frames, and put
//! back together on the other side, so that a program sends and receives values and never sees a byte, and a
//! backend with no notion of a long message still carries any object.
//!
//! Safety of an object. An `Object` has a finite size by construction: the only collection with an impl is
//! `Bounded`, whose capacity is fixed when the program builds it, and every other impl is a number, an array, a
//! tuple, an `Option`, or a struct or enum of these (`#[derive(trame::Object)]`). Its encoding is little-endian, so
//! hosts of different byte order agree. A decode reads only what the encoding holds: it checks every length,
//! every `bool`, every `Option` and enum tag, and ends in `Malformed`, never in undefined behaviour. The tag of an
//! object is a hash of its shape (the names and field types of the type, `Shape`), so two programs that disagree
//! on a type never take each other's objects for their own.

use crate::backend::{Backend, Selected};
use crate::bounded::Bounded;
use crate::io::Full;
use crate::topology::HostId;
use alloc::vec::Vec;
use core::cell::UnsafeCell;
use core::fmt;
use core::sync::atomic::{AtomicBool, Ordering};

/// What an object's decoding found wrong with the bytes it was given.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Malformed(pub &'static str);

impl fmt::Display for Malformed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "malformed object: {}", self.0)
    }
}

/// A value that crosses between leaders (`peer_send`, `peer_recv`, `share`). Implemented for numbers, `bool`, `()`,
/// arrays, tuples, `Option`, `Bounded`, and, by `#[derive(trame::Object)]` or `#[trame::message]`, structs and enums of
/// these. A program does not write the four methods; they are the contract of the layer.
pub trait Object: Sized + 'static {
    /// The exact number of bytes `encode` writes for this value. The sizing metadata of the value: the layer sizes
    /// its buffers and counts its frames from it, and refuses an `encode` that disagrees.
    fn size(&self) -> usize;
    #[doc(hidden)]
    fn encode(&self, w: &mut Writer);
    #[doc(hidden)]
    fn decode(r: &mut Reader<'_>) -> Result<Self, Malformed>;
    /// The layout of the type, fed to the tag: its names and the shapes of its fields.
    #[doc(hidden)]
    fn shape(s: &mut Shape);
}

/// The bytes of an encoding under construction.
pub struct Writer {
    buf: Vec<u8>,
}

impl Writer {
    pub(crate) fn with_capacity(n: usize) -> Self {
        Writer { buf: Vec::with_capacity(n) }
    }
    pub fn put(&mut self, bytes: &[u8]) {
        self.buf.extend_from_slice(bytes);
    }
    pub fn put_u32(&mut self, v: u32) {
        self.put(&v.to_le_bytes());
    }
    pub fn put_u64(&mut self, v: u64) {
        self.put(&v.to_le_bytes());
    }
}

/// The bytes of an encoding being read: each read is checked against what is left.
pub struct Reader<'a> {
    buf: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    pub(crate) fn new(buf: &'a [u8]) -> Self {
        Reader { buf, at: 0 }
    }
    pub fn take(&mut self, n: usize) -> Result<&'a [u8], Malformed> {
        let end = self.at.checked_add(n).filter(|e| *e <= self.buf.len()).ok_or(Malformed("ends early"))?;
        let out = &self.buf[self.at..end];
        self.at = end;
        Ok(out)
    }
    pub fn get_u32(&mut self) -> Result<u32, Malformed> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().map_err(|_| Malformed("ends early"))?))
    }
    pub fn get_u64(&mut self) -> Result<u64, Malformed> {
        Ok(u64::from_le_bytes(self.take(8)?.try_into().map_err(|_| Malformed("ends early"))?))
    }
    pub(crate) fn rest(&self) -> usize {
        self.buf.len() - self.at
    }
}

/// The layout of a type, hashed (FNV-1a) into its tag.
pub struct Shape(u64);

impl Shape {
    fn new() -> Self {
        Shape(0xcbf2_9ce4_8422_2325)
    }
    fn byte(&mut self, b: u8) {
        self.0 = (self.0 ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3);
    }
    /// A name (of a type, a field, a variant), set apart from the next.
    pub fn word(&mut self, w: &str) {
        w.bytes().for_each(|b| self.byte(b));
        self.byte(0xff);
    }
    pub fn number(&mut self, n: u64) {
        n.to_le_bytes().iter().for_each(|b| self.byte(*b));
        self.byte(0xfe);
    }
}

/// The tag of an object type (CONTRACT.md F4): equal in two compilations of the same type, and different for a
/// type whose shape differs.
fn tag<T: Object>() -> u64 {
    let mut s = Shape::new();
    T::shape(&mut s);
    s.0
}

fn encode<T: Object>(obj: &T) -> Vec<u8> {
    let mut w = Writer::with_capacity(obj.size());
    obj.encode(&mut w);
    assert_eq!(w.buf.len(), obj.size(), "trame: `size` of {} disagrees with its encoding", core::any::type_name::<T>());
    w.buf
}

fn decode<T: Object>(bytes: &[u8]) -> T {
    let mut r = Reader::new(bytes);
    let out = T::decode(&mut r).and_then(|v| if r.rest() == 0 { Ok(v) } else { Err(Malformed("bytes left over")) });
    match out {
        Ok(v) => v,
        Err(e) => panic!("trame: {e} (a {})", core::any::type_name::<T>()),
    }
}

macro_rules! number {
    ($($t:ty),*) => {$(
        impl Object for $t {
            fn size(&self) -> usize {
                core::mem::size_of::<$t>()
            }
            fn encode(&self, w: &mut Writer) {
                w.put(&self.to_le_bytes());
            }
            fn decode(r: &mut Reader<'_>) -> Result<Self, Malformed> {
                Ok(<$t>::from_le_bytes(r.take(core::mem::size_of::<$t>())?.try_into().map_err(|_| Malformed("ends early"))?))
            }
            fn shape(s: &mut Shape) {
                s.word(stringify!($t));
            }
        }
    )*};
}
number!(u8, u16, u32, u64, u128, i8, i16, i32, i64, i128, f32, f64);

impl Object for usize {
    fn size(&self) -> usize {
        8
    }
    fn encode(&self, w: &mut Writer) {
        w.put_u64(*self as u64);
    }
    fn decode(r: &mut Reader<'_>) -> Result<Self, Malformed> {
        usize::try_from(r.get_u64()?).map_err(|_| Malformed("a usize too large for this host"))
    }
    fn shape(s: &mut Shape) {
        s.word("usize");
    }
}

impl Object for isize {
    fn size(&self) -> usize {
        8
    }
    fn encode(&self, w: &mut Writer) {
        w.put(&(*self as i64).to_le_bytes());
    }
    fn decode(r: &mut Reader<'_>) -> Result<Self, Malformed> {
        isize::try_from(r.get_u64()? as i64).map_err(|_| Malformed("an isize too large for this host"))
    }
    fn shape(s: &mut Shape) {
        s.word("isize");
    }
}

impl Object for bool {
    fn size(&self) -> usize {
        1
    }
    fn encode(&self, w: &mut Writer) {
        w.put(&[u8::from(*self)]);
    }
    fn decode(r: &mut Reader<'_>) -> Result<Self, Malformed> {
        match r.take(1)?[0] {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(Malformed("a bool that is neither 0 nor 1")),
        }
    }
    fn shape(s: &mut Shape) {
        s.word("bool");
    }
}

impl Object for () {
    fn size(&self) -> usize {
        0
    }
    fn encode(&self, _: &mut Writer) {}
    fn decode(_: &mut Reader<'_>) -> Result<Self, Malformed> {
        Ok(())
    }
    fn shape(s: &mut Shape) {
        s.word("()");
    }
}

impl<T: Object, const N: usize> Object for [T; N] {
    fn size(&self) -> usize {
        self.iter().map(Object::size).sum()
    }
    fn encode(&self, w: &mut Writer) {
        self.iter().for_each(|v| v.encode(w));
    }
    fn decode(r: &mut Reader<'_>) -> Result<Self, Malformed> {
        let mut items = Vec::with_capacity(N);
        for _ in 0..N {
            items.push(T::decode(r)?);
        }
        items.try_into().map_err(|_| Malformed("an array of the wrong length"))
    }
    fn shape(s: &mut Shape) {
        s.word("array");
        s.number(N as u64);
        T::shape(s);
    }
}

impl<T: Object> Object for Option<T> {
    fn size(&self) -> usize {
        1 + self.as_ref().map_or(0, Object::size)
    }
    fn encode(&self, w: &mut Writer) {
        match self {
            None => w.put(&[0]),
            Some(v) => {
                w.put(&[1]);
                v.encode(w);
            }
        }
    }
    fn decode(r: &mut Reader<'_>) -> Result<Self, Malformed> {
        match r.take(1)?[0] {
            0 => Ok(None),
            1 => Ok(Some(T::decode(r)?)),
            _ => Err(Malformed("an Option that is neither none nor some")),
        }
    }
    fn shape(s: &mut Shape) {
        s.word("option");
        T::shape(s);
    }
}

macro_rules! tuple {
    ($($t:ident $i:tt),+) => {
        impl<$($t: Object),+> Object for ($($t,)+) {
            fn size(&self) -> usize {
                0 $(+ self.$i.size())+
            }
            fn encode(&self, w: &mut Writer) {
                $(self.$i.encode(w);)+
            }
            fn decode(r: &mut Reader<'_>) -> Result<Self, Malformed> {
                Ok(($($t::decode(r)?,)+))
            }
            fn shape(s: &mut Shape) {
                s.word("tuple");
                $($t::shape(s);)+
            }
        }
    };
}
tuple!(A 0);
tuple!(A 0, B 1);
tuple!(A 0, B 1, C 2);
tuple!(A 0, B 1, C 2, D 3);

/// A `Bounded` crosses with its capacity, so the receiver builds one of the same capacity: a collection sized when the
/// program was built stays sized, whichever host holds it.
impl<T: Object> Object for Bounded<T> {
    fn size(&self) -> usize {
        16 + self.as_slice().iter().map(Object::size).sum::<usize>()
    }
    fn encode(&self, w: &mut Writer) {
        w.put_u64(self.cap() as u64);
        w.put_u64(self.len() as u64);
        self.as_slice().iter().for_each(|v| v.encode(w));
    }
    fn decode(r: &mut Reader<'_>) -> Result<Self, Malformed> {
        let cap = usize::try_from(r.get_u64()?).map_err(|_| Malformed("a capacity too large for this host"))?;
        let len = usize::try_from(r.get_u64()?).map_err(|_| Malformed("a length too large for this host"))?;
        if len > cap {
            return Err(Malformed("a length above its capacity"));
        }
        if cap.checked_mul(core::mem::size_of::<T>().max(1)).is_none_or(|bytes| bytes > isize::MAX as usize) {
            return Err(Malformed("a capacity that cannot be allocated"));
        }
        let mut out = Bounded::new(cap);
        for _ in 0..len {
            if out.push_owned(T::decode(r)?).is_err() {
                return Err(Malformed("a length above its capacity"));
            }
        }
        Ok(out)
    }
    fn shape(s: &mut Shape) {
        s.word("bounded");
        T::shape(s);
    }
}

// ---- the frame layer ----

/// The bytes of one frame on the peer channel: a header, then the payload, padded to this size so that a backend
/// carries every frame alike.
const FRAME: usize = 16384;
const HEADER: usize = 24;
const PAYLOAD: usize = FRAME - HEADER;
/// The tag every frame travels under on the backend (CONTRACT.md F4): one stream per sender carries every object
/// in the order sent, and the object's own tag is in the frame.
const FRAME_TAG: u64 = 0x7472_616d_6f62_6a31;

/// One object being put back together: frames of it have come, not all.
struct Part {
    tag: u64,
    total: usize,
    bytes: Vec<u8>,
}

/// What this leader has received: objects under assembly, one per sender (a sender's frames of one object are
/// contiguous), and finished objects in the order they completed.
struct Inbox {
    parts: Vec<(u32, Part)>,
    done: Vec<(u32, u64, Vec<u8>)>,
}

/// The inbox is the leader's alone; this lock only keeps a second thread of a program from corrupting it.
struct Locked {
    held: AtomicBool,
    inbox: UnsafeCell<Inbox>,
}

// SAFETY: every access to `inbox` is inside `with`, which holds `held`.
unsafe impl Sync for Locked {}

static INBOX: Locked =
    Locked { held: AtomicBool::new(false), inbox: UnsafeCell::new(Inbox { parts: Vec::new(), done: Vec::new() }) };

fn with<R>(f: impl FnOnce(&mut Inbox) -> R) -> R {
    struct Release;
    impl Drop for Release {
        fn drop(&mut self) {
            INBOX.held.store(false, Ordering::Release);
        }
    }
    while INBOX.held.swap(true, Ordering::Acquire) {
        core::hint::spin_loop();
    }
    let _release = Release;
    // SAFETY: `held` is ours until `_release` drops.
    f(unsafe { &mut *INBOX.inbox.get() })
}

/// Take every frame the backend holds into the inbox.
fn pump() {
    let mut frame = alloc::vec![0u8; FRAME];
    while let Some(from) = Selected::peer_recv(FRAME_TAG, &mut frame) {
        with(|inbox| absorb(inbox, from, &frame));
    }
}

fn absorb(inbox: &mut Inbox, from: u32, frame: &[u8]) {
    let word = |at: usize| u64::from_le_bytes(frame[at..at + 8].try_into().expect("a frame holds its header"));
    let (tag, total) = (word(0), word(8) as usize);
    let len = u32::from_le_bytes(frame[16..20].try_into().expect("a frame holds its header")) as usize;
    assert!(len <= PAYLOAD, "trame: a peer frame claims {len} bytes of payload, at most {PAYLOAD} fit");
    let at = match inbox.parts.iter().position(|(h, _)| *h == from) {
        Some(at) => at,
        None => {
            inbox.parts.push((from, Part { tag, total, bytes: Vec::with_capacity(total) }));
            inbox.parts.len() - 1
        }
    };
    let part = &mut inbox.parts[at].1;
    assert!(
        part.tag == tag && part.total == total,
        "trame: a frame of another object arrived inside an object from host {from}"
    );
    part.bytes.extend_from_slice(&frame[HEADER..HEADER + len]);
    assert!(part.bytes.len() <= total, "trame: an object from host {from} is longer than it said");
    if part.bytes.len() == total {
        let (_, part) = inbox.parts.swap_remove(at);
        inbox.done.push((from, part.tag, part.bytes));
    }
}

/// Send `obj` to the leader of host `to` (CONTRACT.md F6), before a launch or after it: encoded, cut into frames,
/// and handed to the backend frame by frame. The send does not wait for the receiver. `Full` only when the backend
/// takes none of it; an object once started is carried whole. Panics when `to` is not a peer.
pub fn peer_send<T: Object>(to: HostId, obj: &T) -> Result<(), Full> {
    let (tag, bytes) = (tag::<T>(), encode(obj));
    let mut frame = alloc::vec![0u8; FRAME];
    let count = bytes.len().div_ceil(PAYLOAD).max(1);
    for i in 0..count {
        let chunk = &bytes[(i * PAYLOAD).min(bytes.len())..((i + 1) * PAYLOAD).min(bytes.len())];
        frame[..8].copy_from_slice(&tag.to_le_bytes());
        frame[8..16].copy_from_slice(&(bytes.len() as u64).to_le_bytes());
        frame[16..20].copy_from_slice(&(chunk.len() as u32).to_le_bytes());
        frame[20..24].fill(0);
        frame[HEADER..HEADER + chunk.len()].copy_from_slice(chunk);
        frame[HEADER + chunk.len()..].fill(0);
        match Selected::peer_send(to.0, FRAME_TAG, &frame) {
            Ok(()) => {}
            Err(Full) if i == 0 => return Err(Full),
            Err(Full) => panic!("trame: the peer channel refused frame {i} of {count} of an object it had begun"),
        }
    }
    Ok(())
}

/// The next object of type `T` from a peer leader, with the host it came from, or `None` while none has arrived whole.
/// Objects of one type from one peer arrive in the order they were sent. Panics, naming the type, on an object whose
/// bytes are not an encoding of `T`.
pub fn peer_recv<T: Object>() -> Option<(HostId, T)> {
    pump();
    let want = tag::<T>();
    let (from, bytes) = with(|inbox| {
        let i = inbox.done.iter().position(|(_, t, _)| *t == want)?;
        let (from, _, bytes) = inbox.done.remove(i);
        Some((from, bytes))
    })?;
    Some((HostId(from), decode(&bytes)))
}

/// The leader's value, on every process of the host: the leader runs `make` and returns its value, every other
/// process returns the leader's value, decoded. Collective over the host, so every process calls it, in the same order,
/// before its launch or after. `make` runs on the leader only.
pub fn share<T: Object>(make: impl FnOnce() -> T) -> T {
    if Selected::is_leader() {
        let value = make();
        let body = encode(&value);
        let mut sent = Vec::with_capacity(8 + body.len());
        sent.extend_from_slice(&tag::<T>().to_le_bytes());
        sent.extend_from_slice(&body);
        Selected::share(&sent);
        value
    } else {
        let got = Selected::share(&[]);
        assert!(
            got.len() >= 8 && got[..8] == tag::<T>().to_le_bytes(),
            "trame: the leader shared a value of another type than {}",
            core::any::type_name::<T>()
        );
        decode(&got[8..])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round<T: Object + PartialEq + fmt::Debug>(v: T) {
        let bytes = encode(&v);
        assert_eq!(decode::<T>(&bytes), v);
    }

    #[test]
    fn values_round_trip() {
        round(0x1234_5678u32);
        round(-3i64);
        round(1.5f64);
        round(true);
        round(Some(7u8));
        round(None::<u16>);
        round([1u8, 2, 3]);
        round((1u32, false, [9u16; 2]));
        round(());
    }

    #[test]
    fn bounded_keeps_its_capacity() {
        let mut b = Bounded::<u32>::new(10);
        b.push(4).unwrap();
        b.push(5).unwrap();
        let back = decode::<Bounded<u32>>(&encode(&b));
        assert_eq!((back.cap(), back.as_slice()), (10, &[4u32, 5][..]));
    }

    #[test]
    fn malformed_bytes_are_refused() {
        fn err(bytes: &[u8]) -> Reader<'_> {
            Reader::new(bytes)
        }
        assert!(bool::decode(&mut err(&[2])).is_err());
        assert!(Option::<u8>::decode(&mut err(&[2])).is_err());
        assert!(u32::decode(&mut err(&[1, 2])).is_err());
        // a length above its capacity, a capacity that cannot be allocated
        let mut w = Writer::with_capacity(16);
        w.put_u64(1);
        w.put_u64(2);
        assert!(Bounded::<u8>::decode(&mut Reader::new(&w.buf)).is_err());
        let mut w = Writer::with_capacity(16);
        w.put_u64(u64::MAX);
        w.put_u64(0);
        assert!(Bounded::<u64>::decode(&mut Reader::new(&w.buf)).is_err());
    }

    #[derive(crate::Object, PartialEq, Debug)]
    struct Unit;
    #[derive(crate::Object, PartialEq, Debug)]
    struct Pair(u8, Option<bool>);
    #[derive(crate::Object, PartialEq, Debug)]
    enum Shapes {
        Nothing,
        One(u32),
        Two { a: [u8; 2], b: Pair },
    }
    #[derive(crate::Object, Debug)]
    struct Nest {
        name: Bounded<u8>,
        items: Bounded<Shapes>,
        unit: Unit,
    }

    #[test]
    fn derived_types_round_trip_and_refuse_a_bad_tag() {
        let mut name = Bounded::new(8);
        name.extend_from_slice(b"abc").unwrap();
        let mut items = Bounded::new(3);
        for s in [Shapes::Nothing, Shapes::One(9), Shapes::Two { a: [1, 2], b: Pair(3, Some(true)) }] {
            items.push_owned(s).unwrap();
        }
        let nest = Nest { name, items, unit: Unit };
        let bytes = encode(&nest);
        assert_eq!(bytes.len(), nest.size());
        let back = decode::<Nest>(&bytes);
        assert_eq!(encode(&back), bytes);
        assert_eq!((back.items.cap(), back.name.as_slice()), (3, &b"abc"[..]));
        assert_eq!(back.items.as_slice()[2], nest.items.as_slice()[2]);
        // an enum tag that names no variant, and bytes left over, are refused
        assert!(Shapes::decode(&mut Reader::new(&[7, 0, 0, 0])).is_err());
        let mut long = encode(&Pair(1, None));
        long.push(0);
        assert!(std::panic::catch_unwind(|| decode::<Pair>(&long)).is_err());
    }

    #[test]
    fn shape_tells_types_apart() {
        assert_ne!(tag::<Pair>(), tag::<Unit>());
        assert_ne!(tag::<u32>(), tag::<i32>());
        assert_ne!(tag::<[u8; 2]>(), tag::<[u8; 3]>());
        assert_ne!(tag::<Option<u8>>(), tag::<u8>());
    }

    #[test]
    fn frames_put_an_object_back_together() {
        // Cut an encoding as `peer_send` does and feed the frames to `absorb`, which must hand it back whole.
        let bytes: Vec<u8> = (0..(2 * PAYLOAD + 123)).map(|i| (i % 251) as u8).collect();
        let mut inbox = Inbox { parts: Vec::new(), done: Vec::new() };
        let mut frame = alloc::vec![0u8; FRAME];
        for chunk in bytes.chunks(PAYLOAD) {
            frame[..8].copy_from_slice(&5u64.to_le_bytes());
            frame[8..16].copy_from_slice(&(bytes.len() as u64).to_le_bytes());
            frame[16..20].copy_from_slice(&(chunk.len() as u32).to_le_bytes());
            frame[HEADER..HEADER + chunk.len()].copy_from_slice(chunk);
            assert!(inbox.done.is_empty());
            absorb(&mut inbox, 1, &frame);
        }
        assert_eq!(inbox.done.len(), 1);
        assert_eq!((inbox.done[0].0, inbox.done[0].1, &inbox.done[0].2), (1, 5, &bytes));
        assert!(inbox.parts.is_empty());
    }
}
