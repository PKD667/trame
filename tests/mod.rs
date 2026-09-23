// Backend tests, one module per subject.
// Window mapping is unit-testable without a live MPI world. MPI measurements live in
// `trame/experiments/` and `trame/bench.sh`.

// The ownership boundary, in every build and every feature selection. It also checks that the
// device backend names none of the host mechanisms under `cpu/`.
mod boundary;
// `concurrent!` and the step primitives, against whichever backend is selected.
mod step;
// `#[parallel]` through `invoke!` and `concurrent!`'s scheduling, per lowering: host threads here,
// the warp in `nv_declare`.
#[cfg(not(feature = "nv"))]
mod invoke;
#[cfg(feature = "nv")]
mod nv_declare;
#[cfg(all(feature = "ring", not(feature = "rma-lossy")))]
mod rma;
// The device backend, on the host model of the warp. No MPI, no GPU: one process, `nv::peers`'s
// `Fabric` for the links, and a test that installs which rank it is.
#[cfg(feature = "nv")]
mod nv;
