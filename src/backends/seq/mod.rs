/// The sequential backend (CONTRACT.md, Backends in the tree). Its state and operations come below.
pub struct Selected;

mod edit;
mod limit;
/// The routes of this backend: workers share one address space (../local/ is not a backend, it holds the rings).
#[path = "../local/routes.rs"]
mod local_routes;
mod ops;
mod state;
mod stuck;

#[cfg(test)]
mod tests_limit;
// `#[trame::state]` expands to compile_error! unless the build is checked, so the tests that use it
// need that flag.
#[cfg(all(test, trame_checked))]
mod tests;
#[cfg(all(test, trame_checked))]
mod tests_array;
#[cfg(all(test, trame_checked))]
mod tests_launch;
#[cfg(all(test, trame_checked))]
mod tests_leader_stuck;
#[cfg(all(test, trame_checked))]
mod tests_steps;
#[cfg(all(test, trame_checked))]
mod tests_steps_err;
#[cfg(all(test, trame_checked))]
mod tests_steps_io;
#[cfg(all(test, trame_checked))]
mod tests_steps_room;
#[cfg(all(test, trame_checked))]
mod tests_steps_stuck;
#[cfg(all(test, trame_checked))]
mod tests_steps_wait;
#[cfg(all(test, trame_checked))]
mod tests_stuck;
