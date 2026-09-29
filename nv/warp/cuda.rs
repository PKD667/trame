//! The device side of the warp surface: `cuda_device::warp`, with the participation mask named
//! once.
//!
//! These full-warp helpers are not the worker route. Scalar owners never call
//! them after the other lanes leave application entry.

/// This warp's index in the launch.
///
/// Read from the special registers, not `thread::index_1d`: that name is a stub the `#[kernel]`
/// macro rewrites only inside an annotated body, and `init` is not one.
#[inline(always)]
pub fn here_id() -> u32 {
    use cuda_device::thread::{blockDim_x, blockIdx_x, threadIdx_x};
    (blockIdx_x() * blockDim_x() + threadIdx_x()) / super::LANES
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

/// A single resident block contains the entire scalar-owner cohort.
#[inline(always)]
pub(crate) fn cohort(size: u32) -> bool {
    use cuda_device::thread::{blockDim_x, blockDim_y, blockDim_z, gridDim_x, gridDim_y, gridDim_z};
    gridDim_x() == 1 && gridDim_y() == 1 && gridDim_z() == 1
        && blockDim_y() == 1 && blockDim_z() == 1 && blockDim_x() == size * super::LANES
}
