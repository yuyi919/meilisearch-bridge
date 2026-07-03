//! # `@meilisearch-bridge/core` — napi-rs native addon
//!
//! This crate wraps Meilisearch's `milli` search engine crate (vendored under
//! `native/meilisearch/crates/milli`) and exposes it to Node.js via `#[napi]`.
//!
//! Module layout follows the napi-rs-node-bindings skill convention:
//!
//! - `lib.rs`     — crate root, re-exports the public API
//! - `engine.rs`  — `Engine` class (a directory holding multiple indexes)
//! - `index.rs`   — `Index` class (a single searchable index)
//! - `search.rs`  — search result types + search execution
//! - `errors.rs`  — `BridgeError` + `From` impls
//! - `node/mod.rs` — `#[napi]` macro module that wires everything into a `cdylib`
//!
//! Note on threading: napi-rs runs on Node's libuv thread pool. Heavy milli
//! operations (indexing, search) are made async via `#[napi(async)]` so they
//! offload to a tokio runtime and return Promises.

#![deny(rust_2018_idioms)]
#![warn(unused_must_use)]

pub mod engine;
pub mod errors;
pub mod index;
pub mod search;
pub mod task;

pub use engine::Engine;
pub use errors::{BridgeError, BridgeErrorCode, BridgeResult};
pub use index::{GetDocumentsOptions, GetDocumentsResults, Index, IndexSettingsUpdate, SearchOptions};
pub use search::{DocumentHit, SearchResults};
pub use task::{TaskDetails, TaskInfo, TaskStore};
