// The bytes of one publication or one deposit: a POSIX shared-memory object of exactly its length, named
// from the window's name (shm.rs `object`). The creator writes it once, before the window names it; no one
// writes it again. So a reader needs no lock on the object, only the window's word that names it
// (publish.rs, gate.rs). An object is unlinked when the protocol no longer needs it.

extern crate std;

use std::ffi::CString;
use std::format;
use std::fs::File;
use std::io::{self, Read, Write};
use std::os::fd::FromRawFd;
use std::vec;
use std::vec::Vec;

/// Create object `name` holding exactly `bytes`. Fails if the name is taken. On a failed write the
/// object is unlinked again, so a failure leaves nothing behind.
pub(crate) fn create(name: &str, bytes: &[u8]) -> io::Result<()> {
    let mut file = open(name, libc::O_CREAT | libc::O_EXCL | libc::O_RDWR)?;
    let written = file.set_len(bytes.len() as u64).and_then(|()| file.write_all(bytes));
    if let Err(e) = written {
        drop(file);
        let _ = unlink(name);
        return Err(e);
    }
    Ok(())
}

/// The contents of object `name`, which must hold exactly `len` bytes. A size mismatch is an error, so
/// a reader holding a stale length never copies the wrong bytes.
pub(crate) fn read(name: &str, len: usize) -> io::Result<Vec<u8>> {
    let mut file = open(name, libc::O_RDONLY)?;
    let have = file.metadata()?.len();
    if have != len as u64 {
        let text = format!("{name} holds {have} bytes, expected {len}");
        return Err(io::Error::new(io::ErrorKind::InvalidData, text));
    }
    let mut bytes = vec![0u8; len];
    file.read_exact(&mut bytes)?;
    Ok(bytes)
}

/// Unlink object `name`. A name that is already gone counts as unlinked.
pub(crate) fn unlink(name: &str) -> io::Result<()> {
    let c = cstring(name)?;
    // SAFETY: `c` is a valid C string that outlives the call.
    if unsafe { libc::shm_unlink(c.as_ptr()) } == 0 {
        return Ok(());
    }
    let e = io::Error::last_os_error();
    if e.kind() == io::ErrorKind::NotFound { Ok(()) } else { Err(e) }
}

fn open(name: &str, flags: libc::c_int) -> io::Result<File> {
    let c = cstring(name)?;
    // SAFETY: `c` is a valid C string; the mode applies only when O_CREAT is set.
    let fd = unsafe { libc::shm_open(c.as_ptr(), flags, 0o600) };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: shm_open returned a new descriptor, which this File alone owns and closes.
    Ok(unsafe { File::from_raw_fd(fd) })
}

fn cstring(name: &str) -> io::Result<CString> {
    CString::new(name).map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))
}
