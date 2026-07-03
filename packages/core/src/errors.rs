//! Error type for the napi-rs bridge.
//!
//! Following the napi-rs-node-bindings skill pattern: every public method returns
//! `napi::Result<T>` which is `Result<T, napi::Error>`. We convert milli's
//! `crate::error::Error` into a JS `Error` with a code (string) and the original
//! message preserved.

use napi::bindgen_prelude::*;
use napi::JsError;
use std::fmt;

/// JS-facing error codes. These are stable strings that TypeScript code can
/// switch on. They roughly mirror milli's `ErrorKind` variants.
#[derive(Debug, Clone, Copy)]
pub enum BridgeErrorCode {
    Internal,
    InvalidArgument,
    IoError,
    IndexAlreadyExists,
    IndexNotFound,
    DocumentNotFound,
    InvalidDatabaseState,
    SettingsUpdateInvalid,
    SearchError,
    TooManyDocuments,
    OutOfBound,
}

impl fmt::Display for BridgeErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Self::Internal => "Internal",
            Self::InvalidArgument => "InvalidArgument",
            Self::IoError => "IoError",
            Self::IndexAlreadyExists => "IndexAlreadyExists",
            Self::IndexNotFound => "IndexNotFound",
            Self::DocumentNotFound => "DocumentNotFound",
            Self::InvalidDatabaseState => "InvalidDatabaseState",
            Self::SettingsUpdateInvalid => "SettingsUpdateInvalid",
            Self::SearchError => "SearchError",
            Self::TooManyDocuments => "TooManyDocuments",
            Self::OutOfBound => "OutOfBound",
        };
        f.write_str(s)
    }
}

#[derive(Debug)]
pub struct BridgeError {
    pub code: BridgeErrorCode,
    pub message: String,
}

impl fmt::Display for BridgeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for BridgeError {}

impl From<BridgeError> for JsError {
    fn from(e: BridgeError) -> Self {
        // We attach `code` as a string property so JS code can switch on it.
        // Pattern: throw new MeilisearchBridgeError(code, message)
        let mut err = JsError::from_reason(e.message.clone());
        // Attach `code` so JS can introspect. This is the standard napi-rs pattern.
        // SAFETY: `err` is a freshly-constructed JsError; we extend it with a string property.
        unsafe {
            let _ = err.set_named_property("code", e.code.to_string());
        }
        err
    }
}

impl From<milli::Error> for BridgeError {
    fn from(e: milli::Error) -> Self {
        use milli::Error as M;
        // milli v1.48 exposes only 3 top-level variants: InternalError, IoError, UserError.
        // We classify at the top level here, and let JS callers look at `message` for finer detail.
        let code = match &e {
            M::IoError(_) => BridgeErrorCode::IoError,
            M::InternalError(_) => BridgeErrorCode::Internal,
            M::UserError(_) => BridgeErrorCode::InvalidArgument,
        };
        Self { code, message: e.to_string() }
    }
}

impl From<milli::Error> for JsError {
    fn from(e: milli::Error) -> Self {
        BridgeError::from(e).into()
    }
}

impl From<std::io::Error> for BridgeError {
    fn from(e: std::io::Error) -> Self {
        Self { code: BridgeErrorCode::IoError, message: e.to_string() }
    }
}

/// Convenience alias used throughout the crate.
pub type BridgeResult<T> = Result<T, BridgeError>;

// Re-export for use in #[napi] functions which need to return napi::Result<T>.
pub fn into_js<T>(r: BridgeResult<T>) -> napi::Result<T> {
    r.map_err(|e| {
        let msg = e.message.clone();
        let mut err: JsError = e.into();
        // Surface the message as the error reason too (JsError::from_reason already does,
        // but be explicit to make sure).
        let _ = msg; // suppress unused warning when reason was already set
        err
    })
}