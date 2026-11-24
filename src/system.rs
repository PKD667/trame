//! What the launch is on: `trame::system::info()` (INTERFACE.md 6, Definition (Memory of a host)).

use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// The share this host's launch fixed: `None` until a launch has fixed one.
static SHARE: AtomicU64 = AtomicU64::new(0);
static FIXED: AtomicBool = AtomicBool::new(false);

/// A reading of the host this launch is on, the same for every process of the host and fixed at the launch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Info {
    /// The bytes each worker of this host may take from its builder: a lower bound trame guarantees. The host's
    /// available memory at the launch, less what the launch itself holds and a margin trame keeps, less the pool,
    /// divided equally among the host's workers; a builder that takes more than its share is refused at the launch
    /// (`Refused::Share`), so a worker that stays within it is never refused for memory. `None` before the launch,
    /// where the share is not yet fixed, and on a host whose memory the backend cannot read.
    pub worker_bytes: Option<u64>,
}

/// This launch's environment (INTERFACE.md 6, Definition (Memory of a host)). A builder, an edit or a step calls
/// it; the answer is fixed at the launch, so every call in a run gives the same value.
pub fn info() -> Info {
    Info { worker_bytes: fixed() }
}

/// The share the launch fixed, or `None` before one.
pub(crate) fn fixed() -> Option<u64> {
    FIXED.load(Ordering::Acquire).then(|| SHARE.load(Ordering::Relaxed))
}

/// Fix this host's share for the launch that is setting it up: `None` when the backend cannot read the host's memory,
/// which is no share to promise.
pub(crate) fn fix(share: Option<u64>) {
    if let Some(share) = share {
        SHARE.store(share, Ordering::Relaxed);
    }
    FIXED.store(share.is_some(), Ordering::Release);
}

/// Forget the last launch's share when the run ends (`Run::join`), so a later launch reads its own.
pub(crate) fn unfix() {
    FIXED.store(false, Ordering::Release);
}

/// The share of one worker, from what the host has, what the pool took, and how many workers the host has.
/// One rule for every backend: a sixteenth of what is available stays with the launch itself (routes, stamps, the
/// peer channel, the segment's tables), the pool is taken out whole, and the rest is divided equally.
pub(crate) fn share(available: u64, pool_bytes: u64, workers: u32) -> u64 {
    let kept = available - available / 16;
    let left = kept.saturating_sub(pool_bytes);
    if workers == 0 { 0 } else { left / u64::from(workers) }
}

#[cfg(test)]
mod tests {
    use super::share;

    #[test]
    fn the_share_keeps_a_sixteenth_and_takes_the_pool_out_first() {
        assert_eq!(share(16 * 1000, 0, 4), (16 * 1000 - 1000) / 4);
        assert_eq!(share(16 * 1000, 5_000, 2), (15_000 - 5_000) / 2);
        // a pool that eats the margin leaves nothing, never a negative number
        assert_eq!(share(16 * 1000, 99_000, 2), 0);
        assert_eq!(share(16 * 1000, 0, 0), 0);
    }
}
