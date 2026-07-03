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

use std::io::Cursor;
use std::sync::Arc;
use std::time::Instant;

use milli::documents::{DocumentsBatchBuilder, DocumentsBatchReader};
use napi_derive::napi;
use parking_lot::{Mutex, RwLock};
use serde_json::Value;

use crate::errors::{into_js, BridgeError, BridgeErrorCode, BridgeResult};
use crate::search::{DocumentHit, SearchResults};
use crate::task::{TaskDetails, TaskInfo, TaskStore};

#[napi(object)]
#[derive(Clone, Default)]
pub struct IndexSettingsUpdate {
    pub primary_key: Option<String>,
    pub searchable_attributes: Option<Vec<String>>,
    pub displayed_attributes: Option<Vec<String>>,
    pub filterable_attributes: Option<Vec<String>>,
    pub sortable_attributes: Option<Vec<String>>,
}

/// Handle to a single Meilisearch index.
///
/// Cheap to clone (internal state is `Arc<Mutex<...>>`).
#[derive(Clone)]
#[napi]
pub struct Index {
    uid: String,
    primary_key: Arc<RwLock<Option<String>>>,
    inner: Arc<Mutex<milli::Index>>,
    indexer_config: Arc<milli::update::IndexerConfig>,
    task_store: Arc<TaskStore>,
}

#[napi]
impl Index {
    /// Internal constructor used by `Engine::getIndex`. Not exposed to JS
    /// directly — JS callers go through `Engine::getIndex()`.
    #[doc(hidden)]
    pub fn new(
        uid: String,
        primary_key: Option<String>,
        inner: Arc<Mutex<milli::Index>>,
        indexer_config: Arc<milli::update::IndexerConfig>,
        task_store: Arc<TaskStore>,
    ) -> Self {
        Self {
            uid,
            primary_key: Arc::new(RwLock::new(primary_key)),
            inner,
            indexer_config,
            task_store,
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
        self.primary_key.read().clone()
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
    #[napi]
    pub async fn add_documents_from_ndjson(&self, ndjson: String) -> napi::Result<TaskInfo> {
        let inner = self.inner.clone();
        let uid = self.uid.clone();
        let indexer_config = self.indexer_config.clone();
        let task_store = self.task_store.clone();
        let primary_key = self.primary_key.clone();
        let result: BridgeResult<TaskInfo> = tokio::task::spawn_blocking(move || {
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

            let received_documents = parsed.len() as u32;
            let mut builder = DocumentsBatchBuilder::new(Vec::new());
            for obj in &parsed {
                let obj_map = obj.as_object().ok_or_else(|| BridgeError {
                    code: BridgeErrorCode::InvalidArgument,
                    message: "each document must be a JSON object".to_string(),
                })?;
                builder.append_json_object(obj_map).map_err(|e| BridgeError {
                    code: BridgeErrorCode::IoError,
                    message: e.to_string(),
                })?;
            }
            let batch = builder.into_inner().map_err(BridgeError::from)?;
            let reader = DocumentsBatchReader::from_reader(Cursor::new(batch)).map_err(|e| {
                BridgeError {
                    code: BridgeErrorCode::InvalidArgument,
                    message: e.to_string(),
                }
            })?;

            let idx = inner.lock();
            let mut wtxn = idx.write_txn().map_err(BridgeError::from)?;
            let must_stop = milli::MustStopProcessing::default();
            let embedder_stats = std::sync::Arc::new(milli::progress::EmbedderStats::default());
            let ip_policy = http_client::policy::IpPolicy::danger_always_allow();
            let indexer = milli::update::IndexDocuments::new(
                &mut wtxn,
                &idx,
                &indexer_config,
                milli::update::IndexDocumentsConfig::default(),
                |_| {},
                &must_stop,
                &embedder_stats,
                &ip_policy,
            )?;
            let (indexer, indexed) = indexer.add_documents(reader)?;
            let indexed_documents = indexed.map_err(|err| BridgeError {
                code: BridgeErrorCode::InvalidArgument,
                message: err.to_string(),
            })?;
            let _ = indexer.execute()?;
            wtxn.commit().map_err(BridgeError::from)?;
            let resolved_primary_key = idx
                .read_txn()
                .map_err(BridgeError::from)
                .and_then(|rtxn| {
                    idx.primary_key(&rtxn).map_err(BridgeError::from).map(|value| value.map(str::to_owned))
                })?;
            *primary_key.write() = resolved_primary_key;
            task_store.create_succeeded_task(
                Some(uid),
                "documentAdditionOrUpdate",
                Some(TaskDetails {
                    received_documents: Some(received_documents),
                    indexed_documents: Some(indexed_documents as u32),
                    searchable_attributes: None,
                }),
            )
        })
        .await
        .map_err(|e| BridgeError {
            code: BridgeErrorCode::Internal,
            message: format!("add_documents_from_ndjson task panicked: {e}"),
        })?;
        into_js(result)
    }

    #[napi]
    pub async fn update_settings(&self, settings: IndexSettingsUpdate) -> napi::Result<TaskInfo> {
        let inner = self.inner.clone();
        let uid = self.uid.clone();
        let indexer_config = self.indexer_config.clone();
        let task_store = self.task_store.clone();
        let primary_key = self.primary_key.clone();

        let result: BridgeResult<TaskInfo> = tokio::task::spawn_blocking(move || {
            let idx = inner.lock();
            let mut wtxn = idx.write_txn().map_err(BridgeError::from)?;
            let mut update = milli::update::Settings::new(&mut wtxn, &idx, &indexer_config);

            if let Some(value) = settings.primary_key.clone() {
                update.set_primary_key(value);
            }
            if let Some(value) = settings.searchable_attributes.clone() {
                update.set_searchable_fields(value);
            }
            if let Some(value) = settings.displayed_attributes {
                update.set_displayed_fields(value);
            }
            if let Some(value) = settings.filterable_attributes {
                update.set_filterable_fields(
                    value
                        .into_iter()
                        .map(milli::FilterableAttributesRule::Field)
                        .collect(),
                );
            }
            if let Some(value) = settings.sortable_attributes {
                update.set_sortable_fields(value.into_iter().collect());
            }

            update.execute(
                &milli::MustStopProcessing::default(),
                &milli::progress::Progress::default(),
                &http_client::policy::IpPolicy::danger_always_allow(),
                Default::default(),
            )?;

            wtxn.commit().map_err(BridgeError::from)?;
            let resolved_primary_key = idx
                .read_txn()
                .map_err(BridgeError::from)
                .and_then(|rtxn| {
                    idx.primary_key(&rtxn).map_err(BridgeError::from).map(|value| value.map(str::to_owned))
                })?;
            *primary_key.write() = resolved_primary_key;

            task_store.create_succeeded_task(
                Some(uid),
                "settingsUpdate",
                Some(TaskDetails {
                    received_documents: None,
                    indexed_documents: None,
                    searchable_attributes: settings.searchable_attributes,
                }),
            )
        })
        .await
        .map_err(|e| BridgeError {
            code: BridgeErrorCode::Internal,
            message: format!("update_settings task panicked: {e}"),
        })?;
        into_js(result)
    }

    #[napi]
    pub async fn search(&self, query: String) -> napi::Result<SearchResults> {
        let inner = self.inner.clone();
        let primary_key = self.primary_key.clone();
        let result: BridgeResult<SearchResults> = tokio::task::spawn_blocking(move || {
            let started = Instant::now();
            let idx = inner.lock();
            let rtxn = idx.read_txn().map_err(BridgeError::from)?;
            let progress = milli::progress::Progress::default();
            let mut search = milli::Search::new(&rtxn, &idx, &progress);
            if !query.is_empty() {
                search.query(query.clone());
            }
            let executed = search.execute().map_err(|err| BridgeError {
                code: BridgeErrorCode::SearchError,
                message: err.to_string(),
            })?;

            let fields_ids_map = idx.fields_ids_map(&rtxn).map_err(BridgeError::from)?;
            let hits = idx
                .documents(&rtxn, executed.documents_ids.iter().copied())
                .map_err(BridgeError::from)?
                .into_iter()
                .zip(executed.document_scores.iter())
                .map(|((docid, obkv), score_details)| {
                    let attributes = milli::all_obkv_to_json(obkv, &fields_ids_map).map_err(BridgeError::from)?;
                    let id = extract_external_id(
                        &attributes,
                        primary_key.read().clone().as_deref(),
                        docid,
                    );
                    Ok(DocumentHit {
                        id,
                        score: milli::score_details::ScoreDetails::global_score(score_details.iter()),
                        attributes: serde_json::Value::Object(attributes),
                    })
                })
                .collect::<BridgeResult<Vec<_>>>()?;

            Ok(SearchResults {
                estimated_total_hits: executed.candidates.len() as u32,
                processing_time_ms: started.elapsed().as_millis().min(u128::from(u32::MAX)) as u32,
                query: query.clone(),
                is_empty_query: query.is_empty(),
                hits,
            })
        })
        .await
        .map_err(|e| BridgeError {
            code: BridgeErrorCode::Internal,
            message: format!("search task panicked: {e}"),
        })?;
        into_js(result)
    }
}

fn extract_external_id(
    attributes: &serde_json::Map<String, Value>,
    primary_key: Option<&str>,
    fallback_docid: u32,
) -> String {
    primary_key
        .and_then(|field| attributes.get(field))
        .map(value_to_string)
        .unwrap_or_else(|| fallback_docid.to_string())
}

fn value_to_string(value: &Value) -> String {
    match value {
        Value::String(value) => value.clone(),
        Value::Number(value) => value.to_string(),
        Value::Bool(value) => value.to_string(),
        other => other.to_string(),
    }
}
