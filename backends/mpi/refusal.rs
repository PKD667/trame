// Why the MPI transport refuses a launch. Every refusal happens on every rank, before any worker
// starts (spec.md 4, Acceptance): a refusal that only some ranks saw would leave the others waiting.

extern crate std;

use std::fmt;
use std::string::String;

/// Why a launch was refused. Nothing ran.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// The topology breaks a launch check. The string names the check.
    Topology(String),
    /// The processes were given different plans (see `agree`).
    Inconsistent,
    /// The launch needs one process per worker plus the leader; the world has another count.
    Ranks { workers: u32, ranks: u32 },
    /// The window would need more than `LIMIT` bytes.
    Memory { need: usize, limit: usize },
    /// The world is not one shared-memory domain. This backend has no cross-node transport.
    NotOneNode { world: i32, shared: i32 },
    /// The MPI library does not promise the unified memory model that direct loads and stores
    /// on a shared window need.
    NotUnified,
    /// An MPI call failed. The string names the call.
    Mpi(&'static str),
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Refusal::Topology(why) => write!(f, "topology refused: {why}"),
            Refusal::Inconsistent => write!(f, "ranks were given different topologies"),
            Refusal::Ranks { workers, ranks } => {
                write!(f, "{workers} workers need {} ranks, the world has {ranks}", workers + 1)
            }
            Refusal::Memory { need, limit } => {
                write!(f, "window needs {need} bytes, limit is {limit}")
            }
            Refusal::NotOneNode { world, shared } => write!(
                f,
                "world of {world} ranks is not one shared-memory node ({shared} ranks share memory with this one)"
            ),
            Refusal::NotUnified => write!(f, "MPI window is not in the unified memory model"),
            Refusal::Mpi(call) => write!(f, "MPI call {call} failed"),
        }
    }
}

impl std::error::Error for Refusal {}
