/// The sequential backend (spec.md section 6). Its state and operations come below.
pub struct Selected;

mod limit;
mod ops;
mod state;
mod stuck;

#[cfg(test)]
mod tests_limit;
