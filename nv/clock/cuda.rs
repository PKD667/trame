//! The device's clock: the GPU's free-running nanosecond counter.
//!
//! `%globaltimer` counts from the device's own power-up and runs across launches, so two readings
//! from the same device are comparable and the incarnation is the device rather than the launch.
//! What the backend cannot see is a device reset: the counter restarts and nothing in the reading
//! says so, so readings taken either side of a reset are not comparable and this backend cannot
//! tell. That is a stated limitation rather than a silent one.

/// Nanoseconds from the device's counter.
pub fn nanos() -> u64 {
    cuda_device::debug::globaltimer()
}

/// One device, one counter: the comparison domain is the device's power cycle.
pub fn incarnation() -> u32 {
    0
}
