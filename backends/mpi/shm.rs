// The shared window: one `MPI_Win_allocate_shared` on the node communicator, owned by the last rank
// (the leader), and every rank's pointer to it. This is the only file that calls MPI's one-sided
// API. Its futex calls are in futex.rs.
//
// The window is never freed. `MPI_Win_free` is collective, so calling it when a worker returns
// would wait for its peers. The mapping lasts until the process exits.

extern crate std;

use std::ffi::c_void;
use std::sync::atomic::{AtomicU32, AtomicU64};

use mpi::ffi;
use mpi::topology::SimpleCommunicator;
use mpi::traits::*;

use super::refusal::Refusal;

/// MPI_SUCCESS as the `int` the C calls return.
const SUCCESS: i32 = ffi::MPI_SUCCESS as i32;

pub(crate) struct Window {
    win: ffi::MPI_Win,
    base: *mut u8,
    len: usize,
}

impl Window {
    /// Allocate `len` bytes on the last rank of `node`, a communicator whose ranks all share memory,
    /// and open it for direct access. Collective over `node`: every rank calls it, and every rank
    /// gets the same answer back.
    pub(crate) fn allocate(node: &SimpleCommunicator, len: usize) -> Result<Window, Refusal> {
        let raw = node.as_raw();
        // Errors come back as return codes, so one rank's failure cannot abort the others.
        unsafe {
            ffi::MPI_Comm_set_errhandler(raw, ffi::RSMPI_ERRORS_RETURN);
        }
        let owner = node.size() - 1;
        let size: ffi::MPI_Aint = if node.rank() == owner { len as ffi::MPI_Aint } else { 0 };
        let mut base: *mut c_void = std::ptr::null_mut();
        let mut win: ffi::MPI_Win = unsafe { ffi::RSMPI_WIN_NULL };
        let rc = unsafe {
            ffi::MPI_Win_allocate_shared(
                size,
                1,
                ffi::RSMPI_INFO_NULL,
                raw,
                &mut base as *mut *mut c_void as *mut c_void,
                &mut win,
            )
        };
        let local = i32::from(rc == SUCCESS);
        let mut ok = 0;
        node.all_reduce_into(&local, &mut ok, mpi::collective::SystemOperation::min());
        if ok == 0 {
            return Err(Refusal::Mpi("MPI_Win_allocate_shared"));
        }
        // Everyone reads the owner's segment, so every rank uses the same base.
        let mut here: ffi::MPI_Aint = 0;
        let mut unit: i32 = 0;
        let mut ptr: *mut c_void = std::ptr::null_mut();
        let rc = unsafe {
            ffi::MPI_Win_shared_query(
                win,
                owner,
                &mut here,
                &mut unit,
                &mut ptr as *mut *mut c_void as *mut c_void,
            )
        };
        if rc != SUCCESS || ptr.is_null() {
            return Err(Refusal::Mpi("MPI_Win_shared_query"));
        }
        let w = Window { win, base: ptr as *mut u8, len };
        if !w.unified() {
            return Err(Refusal::NotUnified);
        }
        // Passive-target access epoch for the lifetime of the window. Local, never waits.
        let rc = unsafe { ffi::MPI_Win_lock_all(ffi::MPI_MODE_NOCHECK as i32, win) };
        if rc != SUCCESS {
            return Err(Refusal::Mpi("MPI_Win_lock_all"));
        }
        Ok(w)
    }

    fn unified(&self) -> bool {
        let mut val: *mut c_void = std::ptr::null_mut();
        let mut flag: i32 = 0;
        let rc = unsafe {
            ffi::MPI_Win_get_attr(
                self.win,
                ffi::MPI_WIN_MODEL as i32,
                &mut val as *mut *mut c_void as *mut c_void,
                &mut flag,
            )
        };
        if rc != SUCCESS || flag == 0 || val.is_null() {
            return false;
        }
        // MPI_WIN_MODEL's attribute is a pointer to an int.
        let model = unsafe { *(val as *const i32) };
        model == ffi::MPI_WIN_UNIFIED as i32
    }

    pub(crate) fn len(&self) -> usize {
        self.len
    }

    /// Another handle on the same window, for a relay thread (relay.rs). The window is never freed, so
    /// both handles stay valid for the life of the process.
    pub(crate) fn alias(&self) -> Window {
        Window { win: self.win, base: self.base, len: self.len }
    }

    /// The word at `off`. Panics unless `off` is 8-aligned and inside the window: a layout bug
    /// must stop the process, not reach memory outside the window.
    pub(crate) fn word(&self, off: usize) -> &AtomicU64 {
        assert!(off % 8 == 0 && off + 8 <= self.len, "word at {off} outside window of {}", self.len);
        unsafe { &*(self.base.add(off) as *const AtomicU64) }
    }

    /// The 32-bit word at `off`: every epoch and futex word (futex.rs compares 32 bits). Panics unless
    /// `off` is 4-aligned and inside the window.
    pub(crate) fn word32(&self, off: usize) -> &AtomicU32 {
        assert!(off % 4 == 0 && off + 4 <= self.len, "word32 at {off} outside window of {}", self.len);
        unsafe { &*(self.base.add(off) as *const AtomicU32) }
    }

    /// A raw pointer to `off`, for copies the release/acquire pair around them makes ordered.
    pub(crate) fn ptr(&self, off: usize, n: usize) -> *mut u8 {
        assert!(off + n <= self.len, "bytes at {off}+{n} outside window of {}", self.len);
        unsafe { self.base.add(off) }
    }

    /// Zero the whole window. The leader only, before the launch barrier.
    pub(crate) fn zero(&self) {
        unsafe { std::ptr::write_bytes(self.base, 0, self.len) }
    }
}
