// The device machinery's tests, on its host model: a warp is 32 sequential passes, driven by
// `warp::sim`, so the partition `#[parallel]` is lowered to is checked without a GPU. What the
// model cannot stand in for — the collectives that move a value between lanes — is absent rather
// than approximated.
//
// The `cuda` build has no host model to run these on, so they are for the build that does.

#[cfg(not(feature = "cuda"))]
mod declare;
#[cfg(not(feature = "cuda"))]
mod warp;
// `concurrent!`'s turn order on the calling warp.
#[cfg(not(feature = "cuda"))]
mod step;
// The wire's deterministic model of one directed link, the geometry it is cut by, and the host
// transport over a mesh of them.
mod layout;
mod model;
mod transport;
// Both ends of the leader route.
#[cfg(not(feature = "cuda"))]
mod leader;
