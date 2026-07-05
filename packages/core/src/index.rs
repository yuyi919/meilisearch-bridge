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
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

use milli::documents::{DocumentsBatchBuilder, DocumentsBatchReader};
use napi_derive::napi;
use parking_lot::{Mutex, RwLock};
use serde_json::Value;

use crate::errors::{check_not_disposed, into_js, BridgeError, BridgeErrorCode, BridgeResult};
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

#[napi(object)]
#[derive(Clone, Default)]
pub struct GetDocumentsOptions {
    pub offset: Option<u32>,
    pub limit: Option<u32>,
    pub fields: Option<Vec<String>>,
}

#[napi(object)]
#[derive(Clone)]
pub struct GetDocumentsResults {
    pub results: Vec<serde_json::Value>,
    pub offset: u32,
    pub limit: u32,
    pub total: u32,
}

#[napi(object)]
#[derive(Clone, Default)]
pub struct SearchOptions {
    pub offset: Option<u32>,
    pub limit: Option<u32>,
    pub attributes_to_retrieve: Option<Vec<String>>,
}

/// Handle to a single Meilisearch index.
///
/// Cheap to clone (internal state is `Arc<Mutex<...>>`).
#[derive(Clone)]
#[napi]
pub struct Index {
    uid: String,
    primary_key: Arc<RwLock<Option<String>>>,
    /// The underlying `milli::Index` reference, wrapped so that `dispose()`
    /// can drop our strong ref deterministically. Once `dispose()` sets this
    /// to `None`, our handle no longer keeps the LMDB env alive — if every
    /// other handle (and any background thread) has also dropped theirs, the
    /// env closes and its file lock releases. Background tasks spawned by
    /// `add_documents_from_ndjson`/`update_settings` capture their own `Arc`
    /// clone at enqueue time, so they're unaffected by this being `None`.
    inner: Arc<Mutex<Option<Arc<Mutex<milli::Index>>>>>,
    indexer_config: Arc<milli::update::IndexerConfig>,
    task_store: Arc<TaskStore>,
    /// Set to true by `dispose()`. Once set, all methods reject with
    /// `BridgeErrorCode::Disposed`. Each `Index` handle owns its own flag —
    /// disposing one handle does not affect siblings backed by the same
    /// `milli::Index`.
    disposed: Arc<AtomicBool>,
}

impl Index {
    /// Snapshot our `Arc<Mutex<milli::Index>>` clone, or return `Disposed` if
    /// `dispose()` already dropped it. Callers must have already passed
    /// `check_not_disposed` — this is the resource-acquisition half of the
    /// guard, kept separate so the flag check and the `Option` take are
    /// consistent under the inner `Mutex`.
    fn inner_arc(&self) -> BridgeResult<Arc<Mutex<milli::Index>>> {
        match self.inner.lock().clone() {
            Some(arc) => Ok(arc),
            None => Err(BridgeError {
                code: BridgeErrorCode::Disposed,
                message: "cannot use a disposed handle".to_string(),
            }),
        }
    }
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
        disposed: Arc<AtomicBool>,
    ) -> Self {
        Self {
            uid,
            primary_key: Arc::new(RwLock::new(primary_key)),
            inner: Arc::new(Mutex::new(Some(inner))),
            indexer_config,
            task_store,
            disposed,
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

    /// Release this Index handle and prevent further use.
    ///
    /// This only disables *this* handle — sibling handles backed by the same
    /// `milli::Index` (e.g. from another `engine.getIndex()` call) keep
    /// working. Background tasks already spawned by `addDocumentsFromNdjson`
    /// or `updateSettings` hold their own `Arc` clones and run to completion.
    ///
    /// In addition to setting the disposed flag, this drops our strong
    /// reference to the underlying `milli::Index` so that — once every other
    /// handle and any in-flight background thread have also dropped theirs —
    /// the LMDB environment closes and its on-disk lock releases. This is the
    /// deterministic alternative to waiting for JS GC finalization.
    ///
    /// Idempotent — calling it multiple times is safe.
    #[napi]
    pub fn dispose(&self) {
        self.disposed.store(true, Ordering::Release);
        *self.inner.lock() = None;
    }

    /// Total number of documents currently stored in the index.
    #[napi]
    pub fn document_count(&self) -> napi::Result<u32> {
        into_js(check_not_disposed(&self.disposed))?;
        let inner = into_js(self.inner_arc())?;
        let inner = inner.lock();
        let rtxn = into_js(inner.read_txn().map_err(BridgeError::from))?;
        let n: u64 = into_js(inner.number_of_documents(&rtxn).map_err(BridgeError::from))?;
        // Clamp to u32 for JS — practical indexes fit in u32 anyway.
        Ok(n.min(u32::MAX as u64) as u32)
    }

    #[napi]
    pub async fn get_documents(
        &self,
        options: Option<GetDocumentsOptions>,
    ) -> napi::Result<GetDocumentsResults> {
        into_js(check_not_disposed(&self.disposed))?;
        let inner = into_js(self.inner_arc())?;
        let result: BridgeResult<GetDocumentsResults> = tokio::task::spawn_blocking(move || {
            let options = options.unwrap_or_default();
            let offset = options.offset.unwrap_or(0) as usize;
            let limit = options.limit.unwrap_or(20) as usize;

            let idx = inner.lock();
            let rtxn = idx.read_txn().map_err(BridgeError::from)?;
            let total = idx.number_of_documents(&rtxn).map_err(BridgeError::from)?;
            let fields_ids_map = idx.fields_ids_map(&rtxn).map_err(BridgeError::from)?;

            let mut documents = Vec::new();
            for entry in idx
                .all_documents(&rtxn)
                .map_err(BridgeError::from)?
                .skip(offset)
                .take(limit)
            {
                let (_docid, obkv) = entry.map_err(BridgeError::from)?;
                let attributes =
                    milli::all_obkv_to_json(obkv, &fields_ids_map).map_err(BridgeError::from)?;
                documents.push(serde_json::Value::Object(filter_attributes(
                    attributes,
                    options.fields.as_deref(),
                )));
            }

            Ok(GetDocumentsResults {
                results: documents,
                offset: offset.min(u32::MAX as usize) as u32,
                limit: limit.min(u32::MAX as usize) as u32,
                total: total.min(u64::from(u32::MAX)) as u32,
            })
        })
        .await
        .map_err(|e| BridgeError {
            code: BridgeErrorCode::Internal,
            message: format!("get_documents task panicked: {e}"),
        })?;
        into_js(result)
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
        into_js(check_not_disposed(&self.disposed))?;
        let inner = into_js(self.inner_arc())?;
        let uid = self.uid.clone();
        let indexer_config = self.indexer_config.clone();
        let task_store = self.task_store.clone();
        let primary_key = self.primary_key.clone();
        let parsed = into_js(parse_ndjson_documents(&ndjson))?;
        let received_documents = parsed.len() as u32;
        let task = into_js(task_store.create_enqueued_task(
            Some(uid.clone()),
            "documentAdditionOrUpdate",
            Some(TaskDetails {
                received_documents: Some(received_documents),
                indexed_documents: None,
                searchable_attributes: None,
            }),
        ))?;

        let task_uid = task.uid;
        std::thread::spawn(move || {
            let _ = task_store.mark_processing(task_uid);
            let result = process_document_addition(
                inner,
                indexer_config,
                primary_key,
                parsed,
                received_documents,
            );

            match result {
                Ok(details) => {
                    let _ = task_store.mark_succeeded(task_uid, Some(details));
                }
                Err(err) => {
                    let _ = task_store.mark_failed(task_uid, err.to_string());
                }
            }
        });

        Ok(task)
    }

    #[napi]
    pub async fn update_settings(&self, settings: IndexSettingsUpdate) -> napi::Result<TaskInfo> {
        into_js(check_not_disposed(&self.disposed))?;
        let inner = into_js(self.inner_arc())?;
        let uid = self.uid.clone();
        let indexer_config = self.indexer_config.clone();
        let task_store = self.task_store.clone();
        let primary_key = self.primary_key.clone();
        let task = into_js(task_store.create_enqueued_task(
            Some(uid),
            "settingsUpdate",
            Some(TaskDetails {
                received_documents: None,
                indexed_documents: None,
                searchable_attributes: settings.searchable_attributes.clone(),
            }),
        ))?;

        let task_uid = task.uid;
        std::thread::spawn(move || {
            let _ = task_store.mark_processing(task_uid);
            let result = process_settings_update(inner, indexer_config, primary_key, settings);
            match result {
                Ok(details) => {
                    let _ = task_store.mark_succeeded(task_uid, Some(details));
                }
                Err(err) => {
                    let _ = task_store.mark_failed(task_uid, err.to_string());
                }
            }
        });

        Ok(task)
    }

    #[napi]
    pub async fn search(
        &self,
        query: String,
        options: Option<SearchOptions>,
    ) -> napi::Result<SearchResults> {
        into_js(check_not_disposed(&self.disposed))?;
        let inner = into_js(self.inner_arc())?;
        let primary_key = self.primary_key.clone();
        let result: BridgeResult<SearchResults> = tokio::task::spawn_blocking(move || {
            let started = Instant::now();
            let options = options.unwrap_or_default();
            let idx = inner.lock();
            let rtxn = idx.read_txn().map_err(BridgeError::from)?;
            let progress = milli::progress::Progress::default();
            let mut search = milli::Search::new(&rtxn, &idx, &progress);
            if !query.is_empty() {
                search.query(query.clone());
            }
            search.offset(options.offset.unwrap_or(0) as usize);
            search.limit(options.limit.unwrap_or(20) as usize);
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
                    let attributes = milli::all_obkv_to_json(obkv, &fields_ids_map)
                        .map_err(BridgeError::from)?;
                    let id = extract_external_id(
                        &attributes,
                        primary_key.read().clone().as_deref(),
                        docid,
                    );
                    Ok(DocumentHit {
                        id,
                        score: milli::score_details::ScoreDetails::global_score(
                            score_details.iter(),
                        ),
                        attributes: serde_json::Value::Object(filter_attributes(
                            attributes,
                            options.attributes_to_retrieve.as_deref(),
                        )),
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

fn process_document_addition(
    inner: Arc<Mutex<milli::Index>>,
    indexer_config: Arc<milli::update::IndexerConfig>,
    primary_key: Arc<RwLock<Option<String>>>,
    parsed: Vec<Value>,
    received_documents: u32,
) -> BridgeResult<TaskDetails> {
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
    let batch = builder.into_inner().map_err(BridgeError::from)?;
    let reader =
        DocumentsBatchReader::from_reader(Cursor::new(batch)).map_err(|e| BridgeError {
            code: BridgeErrorCode::InvalidArgument,
            message: e.to_string(),
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
    let resolved_primary_key = idx.read_txn().map_err(BridgeError::from).and_then(|rtxn| {
        idx.primary_key(&rtxn)
            .map_err(BridgeError::from)
            .map(|value| value.map(str::to_owned))
    })?;
    *primary_key.write() = resolved_primary_key;

    Ok(TaskDetails {
        received_documents: Some(received_documents),
        indexed_documents: Some(indexed_documents as u32),
        searchable_attributes: None,
    })
}

fn process_settings_update(
    inner: Arc<Mutex<milli::Index>>,
    indexer_config: Arc<milli::update::IndexerConfig>,
    primary_key: Arc<RwLock<Option<String>>>,
    settings: IndexSettingsUpdate,
) -> BridgeResult<TaskDetails> {
    let searchable_attributes = settings.searchable_attributes.clone();
    let idx = inner.lock();
    let mut wtxn = idx.write_txn().map_err(BridgeError::from)?;
    let mut update = milli::update::Settings::new(&mut wtxn, &idx, &indexer_config);

    if let Some(value) = settings.primary_key.clone() {
        update.set_primary_key(value);
    }
    if let Some(value) = searchable_attributes.clone() {
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
    let resolved_primary_key = idx.read_txn().map_err(BridgeError::from).and_then(|rtxn| {
        idx.primary_key(&rtxn)
            .map_err(BridgeError::from)
            .map(|value| value.map(str::to_owned))
    })?;
    *primary_key.write() = resolved_primary_key;

    Ok(TaskDetails {
        received_documents: None,
        indexed_documents: None,
        searchable_attributes,
    })
}

fn parse_ndjson_documents(ndjson: &str) -> BridgeResult<Vec<Value>> {
    let mut parsed = Vec::new();
    for (i, line) in ndjson.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let value: Value = serde_json::from_str(trimmed).map_err(|e| BridgeError {
            code: BridgeErrorCode::InvalidArgument,
            message: format!("line {}: {}", i + 1, e),
        })?;
        if !value.is_object() {
            return Err(BridgeError {
                code: BridgeErrorCode::InvalidArgument,
                message: "each document must be a JSON object".to_string(),
            });
        }
        parsed.push(value);
    }
    Ok(parsed)
}

fn filter_attributes(
    attributes: serde_json::Map<String, Value>,
    fields: Option<&[String]>,
) -> serde_json::Map<String, Value> {
    match fields {
        Some(fields) => fields
            .iter()
            .filter_map(|field| {
                attributes
                    .get(field)
                    .cloned()
                    .map(|value| (field.clone(), value))
            })
            .collect(),
        None => attributes,
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
