//! Everything behind the `bugsink` binary except database plumbing (`bugsink-db`). A library so that the parity
//! harnesses in `examples/` can call into grouping and digest directly.

pub mod api;
pub mod digest;
pub mod grouping;
pub mod ingest;
pub mod protocol;
pub mod quota;
pub mod tasks;
