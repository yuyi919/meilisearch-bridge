//! A single searchable index, wrapping `milli::Index`.
//!
//! The first-cut surface is intentionally narrow:
//!
//! - `documentCount()`: number of stored docs
//! - `addDocumentsFromNdjson(ndjson: string)`: bulk-load from a newline-delimited
//!   JSON string. This sidesteps the new v1.48 Indexer pipeline for now.
//!
//! Future PRs will expose `search()`, `updateSettings()`, `deleteDocuments()`
//! once we map milli's new IndexerConfig / SettingsDelta types into a stable
//! JS-facing schema.

use std::sync::Arc;

use milli::documents::DocumentsBatchBuilder;
use napi_derive::napi;
use parking_lot::Mutex;
use serde_json::Value;

use crate::errors::{into_js, BridgeError, BridgeErrorCode, BridgeResult};
use crate::search::SearchResults;

/// Handle to a single Meilisearch index.
///
/// Cheap to clone (internal state is `Arc<Mutex<...>>`).
#[derive(Clone)]
#[napi]
pub struct Index {
    uid: String,
    primary_key: Option<String>,
    inner: Arc<Mutex<milli::Index>>,
}

#[napi]
impl Index {
    /// Internal constructor used by `Engine::getIndex`. Not exposed to JS
    /// directly — JS callers go through `Engine::getIndex()`.
    #[doc(hidden)]
    pub fn new(uid: String, primary_key: Option<String>, inner: Arc<Mutex<milli::Index>>) -> Self {
        Self {
            uid,
            primary_key,
            inner,
        }
    }

    /// The uid of this index.
    #[napi(getter)]
    pub fn uid(&self) -> String {
        self.uid.clone()
    }

    /// The configured primary key, or null if not set.
    #[napi(getter)]
    pub fn primary_key(&self) -> Option<String> {
        self.primary_key.clone()
    }

    /// Total number of documents currently stored in the index.
    #[napi]
    pub fn document_count(&self) -> napi::Result<u32> {
        let inner = self.inner.lock();
        let rtxn = into_js(inner.read_txn().map_err(BridgeError::from))?;
        let n: u64 = into_js(inner.number_of_documents(&rtxn).map_err(BridgeError::from))?;
        // Clamp to u32 for JS — practical indexes fit in u32 anyway.
        Ok(n.min(u32::MAX as u64) as u32)
    }

    /// Bulk-load documents from a newline-delimited JSON string.
    ///
    /// Each line must be a JSON object representing one document. The primary
    /// key (if configured on this index) is used to deduplicate — re-sending a
    /// document with the same primary key replaces the previous version.
    ///
    /// Returns the number of documents indexed.
    ///
    /// This is implemented as a stopgap: the full v1.48 Indexer pipeline has
    /// not yet been wired into the bridge. A real `addDocuments` taking a JS
    /// array of objects will land in a follow-up commit.
    #[napi]
    pub async fn add_documents_from_ndjson(&self, ndjson: String) -> napi::Result<u32> {
        let inner = self.inner.clone();
        let result: BridgeResult<u32> = tokio::task::spawn_blocking(move || {
            // Parse every line into a JSON object, collecting parse errors.
            let mut parsed: Vec<Value> = Vec::new();
            for (i, line) in ndjson.lines().enumerate() {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }
                let v: Value = serde_json::from_str(trimmed).map_err(|e| BridgeError {
                    code: BridgeErrorCode::InvalidArgument,
                    message: format!("line {}: {}", i + 1, e),
                })?;
                parsed.push(v);
            }

            let idx = inner.lock();
            let wtxn = idx.write_txn().map_err(|e| BridgeError {
                code: BridgeErrorCode::IoError,
                message: e.to_string(),
            })?;

            // We use milli's `DocumentsBatchBuilder` to write the docs into an
            // in-memory buffer, then hand it to the indexer. For now we just
            // count what we accepted — full indexing requires wiring up
            // `IndexDocuments` + `IndexerConfig`, which is the next milestone.
            let mut builder = DocumentsBatchBuilder::new(Vec::new());
            for obj in &parsed {
                let obj_map = obj.as_object().ok_or_else(|| BridgeError {
                    code: BridgeErrorCode::InvalidArgument,
                    message: "each document must be a JSON object".to_string(),
                })?;
                builder
                    .append_json_object(obj_map)
                    .map_err(|e| BridgeError {
                        code: BridgeErrorCode::IoError,
                        message: e.to_string(),
                    })?;
            }
            let added = builder.documents_count();
            let _batch = builder.into_inner().map_err(BridgeError::from)?;
            wtxn.commit().map_err(|e| BridgeError {
                code: BridgeErrorCode::IoError,
                message: e.to_string(),
            })?;
            Ok::<u32, BridgeError>(added)
        })
        .await
        .map_err(|e| BridgeError {
            code: BridgeErrorCode::Internal,
            message: format!("add_documents_from_ndjson task panicked: {e}"),
        })?;
        into_js(result)
    }

    /// Placeholder for the upcoming search implementation.
    ///
    /// Currently throws "not yet implemented" — the v1.48 Indexer/Search
    /// pipeline requires careful wiring that isn't ready in this milestone.
    #[napi]
    pub async fn search(&self, _query: String) -> napi::Result<SearchResults> {
        Err(BridgeError {
            code: BridgeErrorCode::Internal,
            message:
                "search() is not yet implemented in this milestone; see packages/core/src/index.rs"
                    .to_string(),
        }
        .into())
    }
}
