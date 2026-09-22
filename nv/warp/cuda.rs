//! The device side of the warp surface: `cuda_device::warp`, with the participation mask named
//! once.
//!
//! Every collective here is the full-warp form. A sub-warp mask is a real thing on this hardware
//! and it is not something this backend uses: a rank is a whole warp and its lanes are all
//! present at every rendezvous, so `u32::MAX` is not a default, it is the only mask that can
//! arise. A backend that wanted partial warps would have to say where the mask comes from, and
//! that is a second code path rather than an argument to this one.

/// This warp's index in the launch.
#[inline(always)]
pub fn here_id() -> u32 {
    cuda_device::thread::index_1d().get() as u32 / super::LANES
}

/// This lane's index within its warp.
#[inline(always)]
pub fn lane() -> u32 {
    cuda_device::warp::lane_id()
}

/// Every lane of this warp reaches here before any proceeds.
#[inline(always)]
pub fn sync() {
    cuda_device::warp::sync_mask(u32::MAX)
}

/// Whether `pred` holds on any lane of this warp.
#[inline(always)]
pub fn any(pred: bool) -> bool {
    cuda_device::warp::any(pred)
}

/// Whether `pred` holds on every lane of this warp.
#[inline(always)]
pub fn all(pred: bool) -> bool {
    cuda_device::warp::all(pred)
}

/// Bit `k` set for every lane `k` whose `pred` holds.
#[inline(always)]
pub fn ballot(pred: bool) -> u32 {
    cuda_device::warp::ballot(pred)
}

/// The value `src` currently holds on lane `src`.
#[inline(always)]
pub fn shuffle(value: u32, src: u32) -> u32 {
    cuda_device::warp::shuffle(value, src)
}
