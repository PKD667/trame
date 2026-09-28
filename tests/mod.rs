// The tests every backend runs, one module per subject. A backend's own tests live under its own
// module, which is where naming it belongs; the conformance claims every backend answers are
// `trame/conformance/claims.rs`, run by each backend's launcher.

// The ownership boundary, in every build and every feature selection. It also checks that the
// device backend names none of the host mechanisms under `cpu/`.
mod boundary;
// `concurrent!` and the step primitives, against whichever backend is selected.
mod step;
