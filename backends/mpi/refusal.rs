// Why the MPI transport refuses a launch. Every refusal happens on every rank, before any worker
// starts (spec.md 4, Acceptance): a refusal that only some ranks saw would leave the others waiting.

extern crate std;

use std::ffi::c_char;
use std::fmt;
use std::string::String;

use mpi::ffi;

/// Why a launch was refused. Nothing ran.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// The topology breaks a launch check. The string names the check.
    Topology(String),
    /// The processes were given different plans (see `agree`).
    Inconsistent,
    /// The launch needs one process per worker plus the leader; the world has another count.
    Ranks { workers: u32, ranks: u32 },
    /// Shared memory the launch needs (the window, or the initial publication) could not be made.
    /// `need` is the size asked for; `why` is the OS or MPI error, or that another rank failed.
    Memory { need: usize, why: String },
    /// TRAME_MPI_NODE_RANKS is set to something other than a number of ranks per node, 1 or more.
    NodeRanks(String),
    /// A launch over several nodes needs MPI_THREAD_MULTIPLE: the relay threads call MPI concurrently.
    Threads,
    /// The MPI library does not promise the unified memory model that direct loads and stores
    /// on a shared window need.
    NotUnified,
    /// An MPI call failed. The string names the call.
    Mpi(&'static str),
    /// TRAME_MPI_SMALL is set to something other than `p2p` or `rma` (RMA.md, Setting).
    SmallSetting(String),
    /// The launch has small cross-node routes on RMA lanes, and the lanes could not be made. The string
    /// is the error of `Ring::raw`.
    Rma(String),
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Refusal::Topology(why) => write!(f, "topology refused: {why}"),
            Refusal::SmallSetting(text) => write!(f, "TRAME_MPI_SMALL is not p2p or rma: {text:?}"),
            Refusal::Rma(why) => write!(f, "the RMA lanes could not be made: {why}"),
            Refusal::Inconsistent => write!(f, "ranks were given different topologies"),
            Refusal::Ranks { workers, ranks } => {
                write!(f, "{workers} workers need {} ranks, the world has {ranks}", workers + 1)
            }
            Refusal::Memory { need, why } => write!(f, "{need} bytes of shared memory refused: {why}"),
            Refusal::NodeRanks(value) => {
                write!(f, "TRAME_MPI_NODE_RANKS must be 1 or more, got {value:?}")
            }
            Refusal::Threads => write!(f, "a launch over several nodes needs MPI_THREAD_MULTIPLE"),
            Refusal::NotUnified => write!(f, "MPI window is not in the unified memory model"),
            Refusal::Mpi(call) => write!(f, "MPI call {call} failed"),
        }
    }
}

impl std::error::Error for Refusal {}

/// The MPI library's text for a return code.
pub(crate) fn mpi_text(rc: i32) -> String {
    let mut buf = [0u8; ffi::MPI_MAX_ERROR_STRING as usize];
    let mut n: i32 = 0;
    // SAFETY: the buffer holds MPI_MAX_ERROR_STRING bytes, which the library writes at most.
    unsafe { ffi::MPI_Error_string(rc, buf.as_mut_ptr() as *mut c_char, &mut n) };
    let n = usize::try_from(n).unwrap_or(0).min(buf.len());
    String::from_utf8_lossy(&buf[..n]).into_owned()
}
