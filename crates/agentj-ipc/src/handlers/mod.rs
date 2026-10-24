//! Request handlers grouped by protocol domain (see `dispatch.rs` for routing).
//!
//! Each module holds one domain's `impl IpcServer` method block so the route
//! table stays readable and the root file stays small.

pub(crate) mod lifecycle;
