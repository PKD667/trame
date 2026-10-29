// The futex calls of the window: sleeping on a 32-bit word and waking its sleepers. The kernel
// compares exactly the 32 bits of the word at `off`, so every futex word is a u32 (shm.rs `word32`).
// Callers re-check their condition after every return, so a spurious return only costs a re-check.

extern crate std;

use super::shm::Window;

impl Window {
    /// Sleep while the 32-bit word at `off` equals `seen`. May return spuriously, so the caller
    /// re-checks. Without FUTEX_PRIVATE_FLAG, so it works across the processes that map the window.
    pub(crate) fn wait(&self, off: usize, seen: u32) {
        self.wait_for(off, seen, None);
    }

    /// As `wait`, and give up after `timeout` (relative). A timeout returns the same way a wake does.
    pub(crate) fn wait_for(&self, off: usize, seen: u32, timeout: Option<std::time::Duration>) {
        let at = self.ptr(off, 4) as *mut u32;
        let spec = timeout
            .map(|d| libc::timespec { tv_sec: d.as_secs() as libc::time_t, tv_nsec: d.subsec_nanos() as libc::c_long });
        let ts = spec.as_ref().map_or(std::ptr::null(), |t| t as *const libc::timespec);
        let (op, val) = (libc::FUTEX_WAIT as libc::c_long, seen as libc::c_long);
        let rc = unsafe { libc::syscall(libc::SYS_futex, at, op, val, ts, 0, 0) };
        let e = std::io::Error::last_os_error().raw_os_error();
        assert!(
            rc == 0 || e == Some(libc::EAGAIN) || e == Some(libc::EINTR) || e == Some(libc::ETIMEDOUT),
            "futex wait: {e:?}"
        );
    }

    /// Wake every sleeper on the futex word at `off`.
    pub(crate) fn wake(&self, off: usize) {
        let at = self.ptr(off, 4) as *mut u32;
        let op = libc::FUTEX_WAKE as libc::c_long;
        assert!(unsafe { libc::syscall(libc::SYS_futex, at, op, i32::MAX as libc::c_long) } >= 0, "futex wake");
    }
}
