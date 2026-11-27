//! The identifier vocabulary of the surface: hosts, workers and the ends of routes (INTERFACE.md
//! section 4).

use crate::topology::Leader;

/// A host of the federation: one memory domain, one backend, one leader. Fixed by deployment.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct HostId(pub u32);

/// Identity of a worker: its index among its host's workers (0..n on every host), and its host. Made
/// by `Topology::worker` for this host's, and from `(HostId, k)` for worker k of another host, as that
/// host declared it. The leader is not a worker and has no `WorkerId` (`Leader`).
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct WorkerId(pub(crate) u32, pub(crate) u32);

impl core::fmt::Debug for WorkerId {
    /// `WorkerId(k)` for this host's worker k, `WorkerId(k, host h)` for another host's.
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        if self.1 == HERE {
            write!(f, "WorkerId({})", self.0)
        } else {
            write!(f, "WorkerId({}, host {})", self.0, self.1)
        }
    }
}

/// The host field of a worker of this host.
pub(crate) const HERE: u32 = u32::MAX;

impl WorkerId {
    pub(crate) const fn here(k: u32) -> Self {
        WorkerId(k, HERE)
    }
}

impl From<(HostId, u32)> for WorkerId {
    /// Worker `k` of host `h`; this host's own `h` names this host's worker.
    fn from((h, k): (HostId, u32)) -> Self {
        if h == crate::host() { WorkerId::here(k) } else { WorkerId(k, h.0) }
    }
}

/// An end of a route: a worker or this host's leader. Made from `WorkerId`, `(HostId, k)` and
/// `Leader`; programs pass those, not an `End`.
#[doc(hidden)]
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub enum End {
    Worker(WorkerId),
    Leader,
}

impl core::fmt::Debug for End {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            End::Worker(w) => w.fmt(f),
            End::Leader => f.write_str("Leader"),
        }
    }
}

impl From<WorkerId> for End {
    fn from(w: WorkerId) -> End {
        End::Worker(w)
    }
}

impl From<(HostId, u32)> for End {
    fn from(hk: (HostId, u32)) -> End {
        End::Worker(hk.into())
    }
}

impl From<Leader> for End {
    fn from(_: Leader) -> End {
        End::Leader
    }
}
