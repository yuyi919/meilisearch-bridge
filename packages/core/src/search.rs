//! Search execution + result types.

use napi::bindgen_prelude::*;
use napi_derive::napi;
use serde::{Deserialize, Serialize};

/// A single document hit in search results.
///
/// JS shape: `{ id, score, attributes: { ...originalDocFields } }`
#[napi(object)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocumentHit {
    /// Document primary key (string for JS interop; milli uses u128 internally
    /// but we expose the user-supplied string id when available).
    pub id: String,
    /// Ranking score. Higher is more relevant (milli uses BM25 + custom tweaks).
    pub score: f64,
    /// The original document fields as JSON.
    pub attributes: serde_json::Value,
}

/// Full search results.
///
/// Mirrors the relevant subset of meilisearch-js's `SearchResponse`.
#[napi(object)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResults {
    /// Matching documents, sorted by relevance descending.
    pub hits: Vec<DocumentHit>,
    /// Estimated total matches (may be approximate for large indexes).
    pub estimated_total_hits: u32,
    /// Lower bound of processing time in milliseconds.
    pub processing_time_ms: u32,
    /// The query string as executed (after normalization).
    pub query: String,
    /// Whether the query was empty (returns all docs by milli's defaults).
    pub is_empty_query: bool,
}