//! Top-level engine that holds a directory of named indexes.
//!
//! An `Engine` is a thin wrapper over a directory path. Each index is stored
//! in a subdirectory named after the index uid. Under the hood, milli's
//! `Index` opens a LMDB env per directory.
//!
//! JS shape:
//!   `const engine = new Engine('/path/to/data')`
//!   `await engine.createIndex('movies', 'id')`
//!   `const idx = await engine.getIndex('movies')`
//!   `await idx.addDocuments([{ id: '1', title: 'Inception' }])`
//!   `await idx.search('incep')`

use std::path::PathBuf;
use std::sync::Arc;

use napi::bindgen_prelude::*;
use napi_derive::napi;
use parking_lot::Mutex;
use tokio::sync::RwLock;

use crate::errors::{into_js, BridgeError, BridgeErrorCode, BridgeResult};
use crate::index::Index;

/// The top-level container for a collection of named indexes.
///
/// Once constructed, an `Engine` is cheap to clone and can be shared across
/// threads — internal state is wrapped in `Arc` + `Mutex`/`RwLock`.
#[napi]
pub struct Engine {
    /// Directory holding the index subdirectories.
    base_path: PathBuf,
    /// Cache of currently-open `Index` handles, keyed by uid.
    open_indexes: Arc<RwLock<hashbrown::HashMap<String, Arc<Mutex<milli::Index>>>>>,
    /// LMDB env-open options shared by all indexes. We default to a 4 GiB map
    /// size, matching the value meilisearch uses for production.
    env_builder: heed::EnvOpenOptions<heed::WithoutTls>,
    /// Lock guarding filesystem mutation (create/delete index) — LMDB doesn't
    /// tolerate concurrent env creation on the same path.
    fs_lock: Mutex<()>,
}

#[napi]
impl Engine {
    /// Create a new engine rooted at `basePath`. The directory must exist
    /// (we don't `mkdir -p` to avoid surprising the user; pass an explicit
    /// path that has been created).
    #[napi(constructor)]
    pub fn new(base_path: String) -> napi::Result<Self> {
        let path = PathBuf::from(&base_path);
        if !path.is_dir() {
            return Err(BridgeError {
                code: BridgeErrorCode::InvalidArgument,
                message: format!("base_path is not an existing directory: {}", base_path),
            }
            .into());
        }
        let mut env_builder = heed::EnvOpenOptions::new().read_txn_without_tls();
        env_builder.map_size(4 * 1024 * 1024 * 1024); // 4 GiB
        Ok(Self {
            base_path: path,
            open_indexes: Arc::new(RwLock::new(hashbrown::HashMap::new())),
            env_builder,
            fs_lock: Mutex::new(()),
        })
    }

    /// List the UIDs of every index present on disk.
    #[napi]
    pub async fn list_indexes(&self) -> napi::Result<Vec<String>> {
        let base = self.base_path.clone();
        let result: BridgeResult<Vec<String>> = tokio::task::spawn_blocking(move || {
            let mut out = Vec::new();
            for entry in std::fs::read_dir(&base)?.flatten() {
                let p = entry.path();
                if p.is_dir() {
                    if let Some(name) = p.file_name().and_then(|n| n.to_str()) {
                        // Skip anything starting with '.' (e.g. .DS_Store dirs).
                        if !name.starts_with('.') {
                            out.push(name.to_owned());
                        }
                    }
                }
            }
            Ok(out)
        })
        .await
        .map_err(|e| BridgeError {
            code: BridgeErrorCode::Internal,
            message: format!("list_indexes task panicked: {e}"),
        })?;
        into_js(result)
    }

    /// Create a new index with the given `uid` and `primary_key` (the field
    /// name used as the document id). Fails if the index already exists.
    #[napi]
    pub async fn create_index(&self, uid: String, primary_key: String) -> napi::Result<()> {
        let base = self.base_path.clone();
        let builder = self.env_builder.clone();
        let _fs_guard = self.fs_lock.lock();

        let result: BridgeResult<()> = tokio::task::spawn_blocking(move || {
            let index_path = base.join(&uid);
            if index_path.exists() {
                return Err(BridgeError {
                    code: BridgeErrorCode::IndexAlreadyExists,
                    message: format!("Index {uid:?} already exists at {index_path:?}"),
                });
            }
            std::fs::create_dir_all(&index_path)?;
            let _ = milli::Index::new(
                builder,
                &index_path,
                milli::CreateOrOpen::create_without_shards(),
            )?;
            Ok(())
        })
        .await
        .map_err(|e| BridgeError {
            code: BridgeErrorCode::Internal,
            message: format!("create_index task panicked: {e}"),
        })?;
        into_js(result)
    }

    /// Open (or create-if-absent) an index by uid and return a handle.
    ///
    /// `primary_key` is required only when creating — if the index already
    /// exists it's ignored.
    #[napi]
    pub async fn get_index(&self, uid: String, primary_key: Option<String>) -> napi::Result<Index> {
        let base = self.base_path.clone();
        let builder = self.env_builder.clone();
        let open_indexes = self.open_indexes.clone();

        let result: BridgeResult<Index> = tokio::task::spawn_blocking(move || {
            let index_path = base.join(&uid);
            let already_exists = index_path.exists();
            if !already_exists {
                std::fs::create_dir_all(&index_path)?;
            }
            let create_or_open = if already_exists {
                milli::CreateOrOpen::Open
            } else {
                milli::CreateOrOpen::create_without_shards()
            };
            let milli_index = milli::Index::new(builder, &index_path, create_or_open)?;

            // Cache for later reuse. Wrapped in a sync Mutex<milli::Index> so
            // indexers can lock it for the duration of an add_documents call.
            let handle = Arc::new(Mutex::new(milli_index));
            {
                let mut cache = open_indexes.blocking_write();
                cache.insert(uid.clone(), handle.clone());
            }

            Ok(Index::new(uid, primary_key, handle))
        })
        .await
        .map_err(|e| BridgeError {
            code: BridgeErrorCode::Internal,
            message: format!("get_index task panicked: {e}"),
        })?;
        into_js(result)
    }

    /// Delete an index by uid. Removes the on-disk directory and any cached
    /// handle.
    #[napi]
    pub async fn delete_index(&self, uid: String) -> napi::Result<()> {
        let base = self.base_path.clone();
        let _fs_guard = self.fs_lock.lock();
        {
            let mut cache = self.open_indexes.blocking_write();
            cache.remove(&uid);
        }
        let result: BridgeResult<()> = tokio::task::spawn_blocking(move || {
            let index_path = base.join(&uid);
            if !index_path.exists() {
                return Err(BridgeError {
                    code: BridgeErrorCode::IndexNotFound,
                    message: format!("Index {uid:?} does not exist"),
                });
            }
            std::fs::remove_dir_all(&index_path)?;
            Ok(())
        })
        .await
        .map_err(|e| BridgeError {
            code: BridgeErrorCode::Internal,
            message: format!("delete_index task panicked: {e}"),
        })?;
        into_js(result)
    }
}
