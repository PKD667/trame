// A lane's count words in the window (Counts, in mod.rs; detect.rs reads them).

extern crate std;

use core::sync::atomic::AtomicU64;
use core::sync::atomic::Ordering::SeqCst;

use crate::backends::mpi::memory::shm::Window;

/// A count word of the window (Counts, mod.rs): the route's P or C word, written by this end only.
pub(super) struct Count(*const AtomicU64);

impl Count {
    pub(super) fn at(win: &Window, off: usize) -> Count {
        Count(win.ptr(off, 8) as *const AtomicU64)
    }

    pub(super) fn add(&self) {
        // SAFETY: an aligned word of the window, which stays mapped until MPI_Finalize, after the
        // lanes are closed (shm.rs; F5).
        unsafe { &*self.0 }.fetch_add(1, SeqCst);
    }
}
