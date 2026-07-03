//! napi-rs entry point. Generates the JS/TS bindings at build time.

#![deny(clippy::all)]

use napi_derive::napi;

mod engine;
mod errors;
mod index;
mod search;

// Re-export the public types so the generated `.d.ts` exposes them.
#[napi]
pub struct Exports {
    pub(crate) _phantom: (),
}

/// Version constant exposed to JS for sanity checks / migration tooling.
#[napi]
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// The milli / meilisearch version we are wrapping. Updated by the vendoring
/// script when `git subtree pull` brings in a newer tag.
#[napi]
pub const VENDORED_MEILISEARCH_VERSION: &str = "1.48.3";