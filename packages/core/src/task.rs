use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use napi_derive::napi;
use serde::{Deserialize, Serialize};

use crate::errors::{BridgeError, BridgeErrorCode, BridgeResult};

#[napi(object)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskDetails {
    pub received_documents: Option<u32>,
    pub indexed_documents: Option<u32>,
    pub searchable_attributes: Option<Vec<String>>,
}

#[napi(object)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskInfo {
    pub uid: u32,
    pub index_uid: Option<String>,
    pub status: String,
    pub r#type: String,
    pub details: Option<TaskDetails>,
    pub error: Option<String>,
    pub enqueued_at: String,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
}

#[derive(Debug)]
pub struct TaskStore {
    dir: PathBuf,
    next_uid: AtomicU32,
}

impl TaskStore {
    pub fn new(base_path: &Path) -> BridgeResult<Self> {
        let dir = base_path.join(".tasks");
        std::fs::create_dir_all(&dir)?;

        let max_uid = std::fs::read_dir(&dir)?
            .flatten()
            .filter_map(|entry| {
                let path = entry.path();
                if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
                    return None;
                }
                path.file_stem()
                    .and_then(|stem| stem.to_str())
                    .and_then(|stem| stem.parse::<u32>().ok())
            })
            .max()
            .unwrap_or(0);

        Ok(Self {
            dir,
            next_uid: AtomicU32::new(max_uid.saturating_add(1).max(1)),
        })
    }

    pub fn create_enqueued_task(
        &self,
        index_uid: Option<String>,
        task_type: impl Into<String>,
        details: Option<TaskDetails>,
    ) -> BridgeResult<TaskInfo> {
        let uid = self.next_uid.fetch_add(1, Ordering::SeqCst);
        let now = now_string();
        let task = TaskInfo {
            uid,
            index_uid,
            status: "enqueued".to_string(),
            r#type: task_type.into(),
            details,
            error: None,
            enqueued_at: now,
            started_at: None,
            finished_at: None,
        };
        self.persist(&task)?;
        Ok(task)
    }

    pub fn mark_processing(&self, uid: u32) -> BridgeResult<TaskInfo> {
        let mut task = self.get(uid)?;
        task.status = "processing".to_string();
        if task.started_at.is_none() {
            task.started_at = Some(now_string());
        }
        self.persist(&task)?;
        Ok(task)
    }

    pub fn mark_succeeded(&self, uid: u32, details: Option<TaskDetails>) -> BridgeResult<TaskInfo> {
        let mut task = self.get(uid)?;
        let now = now_string();
        task.status = "succeeded".to_string();
        task.error = None;
        if task.started_at.is_none() {
            task.started_at = Some(now.clone());
        }
        if details.is_some() {
            task.details = details;
        }
        task.finished_at = Some(now);
        self.persist(&task)?;
        Ok(task)
    }

    pub fn mark_failed(&self, uid: u32, error: impl Into<String>) -> BridgeResult<TaskInfo> {
        let mut task = self.get(uid)?;
        let now = now_string();
        task.status = "failed".to_string();
        task.error = Some(error.into());
        if task.started_at.is_none() {
            task.started_at = Some(now.clone());
        }
        task.finished_at = Some(now);
        self.persist(&task)?;
        Ok(task)
    }

    pub fn is_terminal(task: &TaskInfo) -> bool {
        matches!(task.status.as_str(), "succeeded" | "failed")
    }

    pub fn get(&self, uid: u32) -> BridgeResult<TaskInfo> {
        let path = self.task_path(uid);
        let raw = std::fs::read(&path).map_err(|err| {
            if err.kind() == std::io::ErrorKind::NotFound {
                BridgeError {
                    code: BridgeErrorCode::TaskNotFound,
                    message: format!("Task {uid} does not exist"),
                }
            } else {
                BridgeError::from(err)
            }
        })?;

        serde_json::from_slice(&raw).map_err(|err| BridgeError {
            code: BridgeErrorCode::Internal,
            message: format!("failed to deserialize task {uid}: {err}"),
        })
    }

    fn persist(&self, task: &TaskInfo) -> BridgeResult<()> {
        let raw = serde_json::to_vec_pretty(task).map_err(|err| BridgeError {
            code: BridgeErrorCode::Internal,
            message: format!("failed to serialize task {}: {err}", task.uid),
        })?;
        std::fs::write(self.task_path(task.uid), raw)?;
        Ok(())
    }

    fn task_path(&self, uid: u32) -> PathBuf {
        self.dir.join(format!("{uid}.json"))
    }
}

fn now_string() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().to_string())
        .unwrap_or_else(|_| "0".to_string())
}
