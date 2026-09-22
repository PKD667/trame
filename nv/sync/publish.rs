//! Family B publication over launch-owned slots.
//!
//! The host family moves owned buffers through a mutex. A device has neither allocator nor
//! parking, so the launch supplies the buffers and six metadata words. The ownership invariant is
//! the same: one index is the writer's private draft, at most one is published, and at most one is
//! held by the reader. Indices move under a short spinlock; `T` never moves or copies, and the
//! writer cannot name the slot a [`Pinned`] value borrows.

use core::cell::Cell as NotSync;
use core::marker::PhantomData;
use core::ops::Deref;
use core::sync::atomic::Ordering;

use super::Cancel;
use super::atomic::AtomicU32;

/// Metadata words required by [`published`].
pub const WORDS: usize = 6;

const LOCK: usize = 0;
const PUBLISHED: usize = 1;
const READER: usize = 2;
const VERSION_LO: usize = 3;
const VERSION_HI: usize = 4;
const CLOSED: usize = 5;
const FREE: u32 = 0;
const TAKEN: u32 = 1;
const NONE: u32 = u32::MAX;

/// Prepare publication metadata before any writer or reader can run.
pub fn init_header(words: &mut [u32]) {
    assert!(
        words.len() >= WORDS,
        "publication metadata needs {WORDS} words"
    );
    words[LOCK] = FREE;
    words[PUBLISHED] = NONE;
    words[READER] = NONE;
    words[VERSION_LO] = 0;
    words[VERSION_HI] = 0;
    words[CLOSED] = 0;
}

/// A committed publication number. Failed attempts do not advance it.
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Debug, Default)]
pub struct Version(u64);

impl Version {
    pub const NONE: Version = Version(0);

    pub const fn count(self) -> u64 {
        self.0
    }
}

/// Why the writer kept its draft rather than publishing it.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Pressure {
    Full,
    Busy,
}

/// Why a bounded reader wait ended without a newer version.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Ended {
    Cancelled,
    Closed,
    Exhausted { attempts: u32 },
}

/// Split one launch-owned publication into its unique writer and reader.
///
/// # Safety
///
/// `words` must address [`WORDS`] words prepared by [`init_header`]. `values` must address
/// `slots` initialized `T`s, with `slots >= 2`. Both regions must remain alive and visible to all
/// calling warps until both handles are gone. There may be no other access to the values.
pub unsafe fn published<T>(words: *mut u32, values: *mut T, slots: u32) -> (Writer<T>, Reader<T>) {
    assert!(slots >= 2, "a publication needs at least two slots");
    (unsafe { Writer::new(words, values, slots) }, unsafe {
        Reader::new(words, values, slots)
    })
}

struct Common<T> {
    words: *mut u32,
    values: *mut T,
    slots: u32,
}

impl<T> Copy for Common<T> {}

impl<T> Clone for Common<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Common<T> {
    fn atomic(&self, at: usize) -> &AtomicU32 {
        // SAFETY: the constructor requires the metadata region to remain valid.
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

    /// Plain metadata is protected by `LOCK`; publication of its writes is the lock's release.
    unsafe fn get(&self, at: usize) -> u32 {
        unsafe { self.words.add(at).read() }
    }

    unsafe fn set(&self, at: usize, value: u32) {
        unsafe { self.words.add(at).write(value) }
    }

    unsafe fn version(&self) -> Version {
        let lo = unsafe { self.get(VERSION_LO) } as u64;
        let hi = unsafe { self.get(VERSION_HI) } as u64;
        Version(lo | hi << 32)
    }

    unsafe fn set_version(&self, version: Version) {
        unsafe {
            self.set(VERSION_LO, version.0 as u32);
            self.set(VERSION_HI, (version.0 >> 32) as u32);
        }
    }
}

/// The unique writer. Its draft is ordinary exclusive storage.
pub struct Writer<T> {
    common: Common<T>,
    draft: u32,
    committed: Version,
    refused: u64,
    busy: u64,
    overwritten: u64,
    alone: PhantomData<NotSync<()>>,
}

unsafe impl<T: Send> Send for Writer<T> {}

impl<T> Writer<T> {
    /// Open the unique writer endpoint over prepared publication storage.
    ///
    /// # Safety
    ///
    /// As [`published`], and no other writer endpoint may exist for these regions.
    pub unsafe fn new(words: *mut u32, values: *mut T, slots: u32) -> Self {
        assert!(slots >= 2, "a publication needs at least two slots");
        Writer {
            common: Common {
                words,
                values,
                slots,
            },
            draft: 0,
            committed: Version::NONE,
            refused: 0,
            busy: 0,
            overwritten: 0,
            alone: PhantomData,
        }
    }

    pub fn draft(&mut self) -> &mut T {
        // SAFETY: `draft` is excluded from both shared indices while this handle exists.
        unsafe { &mut *self.common.values.add(self.draft as usize) }
    }

    /// Publish without waiting for reader bookkeeping.
    pub fn publish(&mut self) -> Result<Version, Pressure> {
        let next = self
            .committed
            .0
            .checked_add(1)
            .expect("publication: the version counter is exhausted");
        if !self.common.try_lock() {
            self.busy = self.busy.saturating_add(1);
            return Err(Pressure::Busy);
        }
        let result = unsafe {
            let old_published = self.common.get(PUBLISHED);
            let reader = self.common.get(READER);
            let spare = if old_published != NONE {
                old_published
            } else {
                match (0..self.common.slots).find(|&slot| slot != self.draft && slot != reader) {
                    Some(slot) => slot,
                    None => {
                        self.refused = self.refused.saturating_add(1);
                        self.common.unlock();
                        return Err(Pressure::Full);
                    }
                }
            };
            self.common.set(PUBLISHED, self.draft);
            self.common.set_version(Version(next));
            self.draft = spare;
            self.committed = Version(next);
            if old_published != NONE {
                self.overwritten = self.overwritten.saturating_add(1);
            }
            Version(next)
        };
        self.common.unlock();
        Ok(result)
    }

    pub fn refused(&self) -> u64 {
        self.refused
    }

    pub fn busy(&self) -> u64 {
        self.busy
    }

    pub fn overwritten(&self) -> u64 {
        self.overwritten
    }

    pub fn version(&self) -> Version {
        self.committed
    }
}

impl<T> Drop for Writer<T> {
    fn drop(&mut self) {
        self.common.atomic(CLOSED).store(1, Ordering::Release);
    }
}

/// The unique reader. A taken slot remains its property until a later take or drop.
pub struct Reader<T> {
    common: Common<T>,
    held: u32,
    version: Version,
    alone: PhantomData<NotSync<()>>,
}

unsafe impl<T: Send> Send for Reader<T> {}

impl<T> Reader<T> {
    /// Open the unique reader endpoint over prepared publication storage.
    ///
    /// # Safety
    ///
    /// As [`published`], and no other reader endpoint may exist for these regions.
    pub unsafe fn new(words: *mut u32, values: *mut T, slots: u32) -> Self {
        assert!(slots >= 2, "a publication needs at least two slots");
        Reader {
            common: Common {
                words,
                values,
                slots,
            },
            held: NONE,
            version: Version::NONE,
            alone: PhantomData,
        }
    }

    /// Inspect the latest version after at most `attempts` lock attempts. Zero means unbounded.
    pub fn latest(&mut self, attempts: u32) -> Result<Option<Pinned<'_, T>>, Ended> {
        self.step(self.version, attempts)?;
        Ok(self.pinned())
    }

    /// Wait for a version after `seen`, bounded unless `attempts` is zero.
    pub fn after(
        &mut self,
        seen: Version,
        attempts: u32,
        cancel: &Cancel,
    ) -> Result<Pinned<'_, T>, Ended> {
        let mut spent = 0;
        loop {
            if cancel.raised() {
                return Err(Ended::Cancelled);
            }
            match self.step(seen, 1) {
                Ok(true) => return Ok(self.pinned().expect("a newer version was just taken")),
                Ok(false) => {}
                Err(Ended::Closed) => return Err(Ended::Closed),
                Err(Ended::Exhausted { .. }) => {}
                Err(Ended::Cancelled) => unreachable!("step does not inspect cancellation"),
            }
            if attempts > 0 && spent >= attempts {
                return Err(Ended::Exhausted { attempts });
            }
            spent += 1;
            super::spin();
        }
    }

    pub fn version(&self) -> Version {
        self.version
    }

    /// Hold the metadata lock so a writer's `Busy` answer is deterministic.
    #[cfg(test)]
    pub(crate) fn hold(&self) -> impl Drop + '_ {
        self.common.lock();
        Held {
            common: self.common,
        }
    }

    /// `Ok(true)` means a newer slot is held, `Ok(false)` means no newer slot exists.
    fn step(&mut self, seen: Version, attempts: u32) -> Result<bool, Ended> {
        let mut spent = 0;
        while !self.common.try_lock() {
            if attempts > 0 && spent >= attempts {
                return Err(Ended::Exhausted { attempts });
            }
            spent += 1;
            super::spin();
        }
        let took = unsafe {
            let version = self.common.version();
            let published = self.common.get(PUBLISHED);
            if version > seen && published != NONE {
                self.held = published;
                self.version = version;
                self.common.set(READER, published);
                self.common.set(PUBLISHED, NONE);
                true
            } else {
                false
            }
        };
        self.common.unlock();
        if took {
            Ok(true)
        } else if self.common.atomic(CLOSED).load(Ordering::Acquire) != 0 {
            Err(Ended::Closed)
        } else {
            Ok(false)
        }
    }

    fn pinned(&self) -> Option<Pinned<'_, T>> {
        (self.held != NONE).then(|| Pinned {
            // SAFETY: `held` remains the reader index until this mutable reader takes another.
            value: unsafe { &*self.common.values.add(self.held as usize) },
            version: self.version,
        })
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

impl<T> Drop for Reader<T> {
    fn drop(&mut self) {
        if self.held == NONE {
            return;
        }
        self.common.lock();
        unsafe { self.common.set(READER, NONE) };
        self.common.unlock();
    }
}

/// A coherent committed slot borrowed from the reader that owns it.
pub struct Pinned<'a, T> {
    value: &'a T,
    version: Version,
}

impl<T> Pinned<'_, T> {
    pub fn version(&self) -> Version {
        self.version
    }
}

impl<T> Deref for Pinned<'_, T> {
    type Target = T;

    fn deref(&self) -> &T {
        self.value
    }
}
