/// The sequential backend (spec.md section 6). Its state and operations come below.
pub struct Selected;

mod state;
mod ops;

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_barrier;
#[cfg(test)]
mod tests_stuck;
#[cfg(test)]
mod tests_launch;
#[cfg(test)]
mod tests_array;
// `concurrent!` expands to compile_error! unless the build is checked, so its tests need that flag.
#[cfg(all(test, trame_checked))]
mod tests_concurrent;
#[cfg(all(test, trame_checked))]
mod tests_concurrent_err;
#[cfg(all(test, trame_checked))]
mod tests_concurrent_wait;
#[cfg(all(test, trame_checked))]
mod tests_concurrent_io;
#[cfg(all(test, trame_checked))]
mod tests_concurrent_stuck;
#[cfg(all(test, trame_checked))]
mod tests_concurrent_refuse;
#[cfg(all(test, trame_checked))]
mod tests_concurrent_room;
