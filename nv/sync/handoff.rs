//! Family B reliable ownership transfer over fixed launch-owned storage.
//!
//! The launch supplies two arrays of `depth` slots: a FIFO for submitted payloads and a stack for
//! storage returned to the producer. A short metadata lock moves ownership by changing counts and
//! indices; payloads themselves are written once into an uninitialized slot and read once out of
//! it. Every working-path call attempts the lock once, so a descheduled peer is reported as
//! `Busy` rather than waited for.

use core::cell::Cell as NotSync;
use core::marker::PhantomData;
use core::mem::MaybeUninit;
use core::sync::atomic::Ordering;

use super::atomic::AtomicU32;

/// Metadata words required by [`handoff`] and [`stocked`].
pub const WORDS: usize = 6;

const LOCK: usize = 0;
const FULL_HEAD: usize = 1;
const FULL_LEN: usize = 2;
const FREE_LEN: usize = 3;
const SENDING: usize = 4;
const RECEIVING: usize = 5;
const FREE: u32 = 0;
const TAKEN: u32 = 1;

/// Prepare handoff metadata before either endpoint can run.
pub fn init_header(words: &mut [u32]) {
    assert!(words.len() >= WORDS, "handoff metadata needs {WORDS} words");
    words[LOCK] = FREE;
    words[FULL_HEAD] = 0;
    words[FULL_LEN] = 0;
    words[FREE_LEN] = 0;
    words[SENDING] = 1;
    words[RECEIVING] = 1;
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Refused {
    Full,
    Busy,
    Closed,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Unsent<T> {
    pub why: Refused,
    pub value: T,
}

impl<T> Unsent<T> {
    pub fn into_inner(self) -> T {
        self.value
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Idle {
    Empty,
    Busy,
    Closed,
}

/// Open an empty handoff over caller-owned arrays.
///
/// # Safety
///
/// `words` must address [`WORDS`] words prepared by [`init_header`]. `full` and `free` must each
/// address `depth` `MaybeUninit<T>` slots, with `depth >= 1`. The regions must remain alive and
/// otherwise untouched until both handles are gone.
pub unsafe fn handoff<T>(
    words: *mut u32,
    full: *mut MaybeUninit<T>,
    free: *mut MaybeUninit<T>,
    depth: u32,
) -> (Sender<T>, Receiver<T>) {
    assert!(depth >= 1, "a handoff needs room for at least one payload");
    split(Common {
        words,
        full,
        free,
        depth,
    })
}

/// Open a handoff whose entire returned-spare array is already initialized.
///
/// # Safety
///
/// As [`handoff`], and every one of the `depth` slots at `free` must contain a valid `T` whose
/// ownership is transferred to the handoff.
pub unsafe fn stocked<T>(
    words: *mut u32,
    full: *mut MaybeUninit<T>,
    free: *mut MaybeUninit<T>,
    depth: u32,
) -> (Sender<T>, Receiver<T>) {
    assert!(depth >= 1, "a handoff needs room for at least one payload");
    // SAFETY: construction is exclusive and the caller promised every free slot initialized.
    unsafe { words.add(FREE_LEN).write(depth) };
    split(Common {
        words,
        full,
        free,
        depth,
    })
}

fn split<T>(common: Common<T>) -> (Sender<T>, Receiver<T>) {
    (
        Sender {
            common,
            alone: PhantomData,
        },
        Receiver {
            common,
            alone: PhantomData,
        },
    )
}

struct Common<T> {
    words: *mut u32,
    full: *mut MaybeUninit<T>,
    free: *mut MaybeUninit<T>,
    depth: u32,
}

impl<T> Copy for Common<T> {}

impl<T> Clone for Common<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Common<T> {
    fn atomic(&self, at: usize) -> &AtomicU32 {
        // SAFETY: each constructor requires the metadata region to remain valid.
        unsafe { AtomicU32::from_ptr(self.words.add(at)) }
    }

    fn try_lock(&self) -> bool {
        self.atomic(LOCK)
            .compare_exchange(FREE, TAKEN, Ordering::Acquire, Ordering::Relaxed)
            .is_ok()
    }

    fn lock(&self) {
        while !self.try_lock() {
            super::spin();
        }
    }

    fn unlock(&self) {
        self.atomic(LOCK).store(FREE, Ordering::Release);
    }

    unsafe fn get(&self, at: usize) -> u32 {
        unsafe { self.words.add(at).read() }
    }

    unsafe fn set(&self, at: usize, value: u32) {
        unsafe { self.words.add(at).write(value) }
    }

    fn live(&self, at: usize) -> bool {
        self.atomic(at).load(Ordering::Acquire) != 0
    }
}

pub struct Sender<T> {
    common: Common<T>,
    alone: PhantomData<NotSync<()>>,
}

unsafe impl<T: Send> Send for Sender<T> {}

impl<T> Sender<T> {
    /// Open the unique sending endpoint over prepared handoff storage.
    ///
    /// # Safety
    ///
    /// As [`handoff`], and no other sender endpoint may exist for these regions.
    pub unsafe fn new(
        words: *mut u32,
        full: *mut MaybeUninit<T>,
        free: *mut MaybeUninit<T>,
        depth: u32,
    ) -> Self {
        assert!(depth >= 1, "a handoff needs room for at least one payload");
        Sender {
            common: Common {
                words,
                full,
                free,
                depth,
            },
            alone: PhantomData,
        }
    }

    pub fn send(&mut self, value: T) -> Result<(), Unsent<T>> {
        if !self.common.live(RECEIVING) {
            return Err(Unsent {
                why: Refused::Closed,
                value,
            });
        }
        if !self.common.try_lock() {
            return Err(Unsent {
                why: Refused::Busy,
                value,
            });
        }
        if !self.common.live(RECEIVING) {
            self.common.unlock();
            return Err(Unsent {
                why: Refused::Closed,
                value,
            });
        }
        unsafe {
            let len = self.common.get(FULL_LEN);
            if len == self.common.depth {
                self.common.unlock();
                return Err(Unsent {
                    why: Refused::Full,
                    value,
                });
            }
            let head = self.common.get(FULL_HEAD);
            let tail = (head + len) % self.common.depth;
            self.common
                .full
                .add(tail as usize)
                .write(MaybeUninit::new(value));
            self.common.set(FULL_LEN, len + 1);
        }
        self.common.unlock();
        Ok(())
    }

    pub fn spare(&mut self) -> Result<T, Idle> {
        let live = self.common.live(RECEIVING);
        if !self.common.try_lock() {
            return Err(Idle::Busy);
        }
        let result = unsafe {
            let len = self.common.get(FREE_LEN);
            if len == 0 {
                Err(if live && self.common.live(RECEIVING) {
                    Idle::Empty
                } else {
                    Idle::Closed
                })
            } else {
                self.common.set(FREE_LEN, len - 1);
                Ok(self
                    .common
                    .free
                    .add((len - 1) as usize)
                    .read()
                    .assume_init())
            }
        };
        self.common.unlock();
        result
    }

    pub fn taking(&self) -> bool {
        self.common.live(RECEIVING)
    }

    pub fn pending(&self) -> usize {
        self.common.lock();
        let len = unsafe { self.common.get(FULL_LEN) as usize };
        self.common.unlock();
        len
    }
}

impl<T> Drop for Sender<T> {
    fn drop(&mut self) {
        self.common.atomic(SENDING).store(0, Ordering::Release);
        self.common.lock();
        unsafe {
            let len = self.common.get(FREE_LEN);
            for at in 0..len {
                self.common.free.add(at as usize).read().assume_init_drop();
            }
            self.common.set(FREE_LEN, 0);
        }
        self.common.unlock();
    }
}

pub struct Receiver<T> {
    common: Common<T>,
    alone: PhantomData<NotSync<()>>,
}

unsafe impl<T: Send> Send for Receiver<T> {}

impl<T> Receiver<T> {
    /// Open the unique receiving endpoint over prepared handoff storage.
    ///
    /// # Safety
    ///
    /// As [`handoff`], and no other receiver endpoint may exist for these regions.
    pub unsafe fn new(
        words: *mut u32,
        full: *mut MaybeUninit<T>,
        free: *mut MaybeUninit<T>,
        depth: u32,
    ) -> Self {
        assert!(depth >= 1, "a handoff needs room for at least one payload");
        Receiver {
            common: Common {
                words,
                full,
                free,
                depth,
            },
            alone: PhantomData,
        }
    }

    pub fn recv(&mut self) -> Result<T, Idle> {
        let live = self.common.live(SENDING);
        if !self.common.try_lock() {
            return Err(Idle::Busy);
        }
        let result = unsafe {
            let len = self.common.get(FULL_LEN);
            if len == 0 {
                Err(if live && self.common.live(SENDING) {
                    Idle::Empty
                } else {
                    Idle::Closed
                })
            } else {
                let head = self.common.get(FULL_HEAD);
                let value = self.common.full.add(head as usize).read().assume_init();
                self.common.set(FULL_HEAD, (head + 1) % self.common.depth);
                self.common.set(FULL_LEN, len - 1);
                Ok(value)
            }
        };
        self.common.unlock();
        result
    }

    pub fn give(&mut self, value: T) -> Result<(), Unsent<T>> {
        if !self.common.live(SENDING) {
            return Err(Unsent {
                why: Refused::Closed,
                value,
            });
        }
        if !self.common.try_lock() {
            return Err(Unsent {
                why: Refused::Busy,
                value,
            });
        }
        if !self.common.live(SENDING) {
            self.common.unlock();
            return Err(Unsent {
                why: Refused::Closed,
                value,
            });
        }
        unsafe {
            let len = self.common.get(FREE_LEN);
            if len == self.common.depth {
                self.common.unlock();
                return Err(Unsent {
                    why: Refused::Full,
                    value,
                });
            }
            self.common
                .free
                .add(len as usize)
                .write(MaybeUninit::new(value));
            self.common.set(FREE_LEN, len + 1);
        }
        self.common.unlock();
        Ok(())
    }

    pub fn sending(&self) -> bool {
        self.common.live(SENDING)
    }

    pub fn spares(&self) -> usize {
        self.common.lock();
        let len = unsafe { self.common.get(FREE_LEN) as usize };
        self.common.unlock();
        len
    }

    /// Hold the metadata lock so a sender's `Busy` answer is deterministic.
    #[cfg(test)]
    pub(crate) fn hold(&self) -> impl Drop + '_ {
        self.common.lock();
        Held {
            common: self.common,
        }
    }
}

#[cfg(test)]
struct Held<T> {
    common: Common<T>,
}

#[cfg(test)]
impl<T> Drop for Held<T> {
    fn drop(&mut self) {
        self.common.unlock();
    }
}

impl<T> Drop for Receiver<T> {
    fn drop(&mut self) {
        self.common.atomic(RECEIVING).store(0, Ordering::Release);
        self.common.lock();
        unsafe {
            let head = self.common.get(FULL_HEAD);
            let len = self.common.get(FULL_LEN);
            for offset in 0..len {
                let at = (head + offset) % self.common.depth;
                self.common.full.add(at as usize).read().assume_init_drop();
            }
            self.common.set(FULL_LEN, 0);
        }
        self.common.unlock();
    }
}
