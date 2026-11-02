//! Host-only checks for the NVIDIA backend (PLAN.md). Nothing here runs on a GPU, and nothing here
//! is wired into `trame`: the surface would fill `PlanView` from its `Plan` (see `representable`).
pub mod representable;
