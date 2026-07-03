# REST Embedder For OpenRouter And GLM Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add phased embedders support so `@meilisearch-bridge/api` can configure a REST embedder for OpenRouter or GLM, index documents through `milli`, run hybrid search, expose `retrieveVectors` in the base search shape, and reserve `similar` for a follow-up phase.

**Architecture:** Keep the public TypeScript API close to `meilisearch-js` by exposing `embedders` on `updateSettings()`, `hybrid` on `search()`, and `retrieveVectors` in the first-phase search result contract. Keep the N-API boundary narrow by serializing the embedder map as JSON in the API wrapper, then parse and apply it in `packages/core` using `milli`'s `EmbeddingSettings`, `RuntimeEmbedders`, `Search::semantic(...).execute_hybrid(...)`, and `Search::retrieve_vectors(...)`.

**Tech Stack:** TypeScript, Node test runner, napi-rs, Rust, vendored `milli`, local HTTP test server

---

## File structure

- Create: `packages/core/src/embedders.rs`
  - Parse public embedder JSON into `milli` settings.
  - Build runtime embedders from stored `milli` embedding configs.
- Modify: `packages/core/src/lib.rs`
  - Export the new `embedders` module.
- Modify: `packages/core/src/index.rs`
  - Extend `IndexSettingsUpdate` and `SearchOptions`.
  - Apply embedder settings during `update_settings`.
  - Build runtime embedders for indexing and hybrid search.
- Modify: `packages/api/src/index.ts`
  - Add public REST embedder types, `hybrid` search options, and an OpenAI-compatible helper for OpenRouter/GLM.
  - Translate public shapes into the narrower native DTOs.
- Create: `packages/api/__test__/helpers/openai-compatible-embedder.ts`
  - Start a deterministic local embedding server with an OpenAI-compatible `/v1/embeddings` endpoint.
- Modify: `packages/api/__test__/engine.test.ts`
  - Add integration tests for REST embedder settings, document indexing, and hybrid search.
- Modify: `packages/core/index.d.ts`
  - Regenerate after native exports change.
- Modify: `packages/core/index.js`
  - Regenerate after native exports change.

### Scope guard

This plan is split into two phases:

- Phase 1
  - REST embedders
  - OpenAI-compatible helper for OpenRouter/GLM
  - document indexing with runtime embedders
  - hybrid search
  - `retrieveVectors` in search results
- Phase 2
  - `similar`

This plan intentionally does **not** add in Phase 1:

- `similar` endpoints
- multimodal fragments
- full server-style settings read APIs
- non-REST embedders in the public bridge API

### Public API shape for this phase

The TypeScript SDK should expose these public shapes:

```ts
export interface RestEmbedderSettings {
  source: 'rest';
  url: string;
  apiKey?: string;
  dimensions?: number;
  documentTemplate?: string;
  documentTemplateMaxBytes?: number;
  request: Record<string, unknown>;
  response: Record<string, unknown>;
  headers?: Record<string, string>;
}

export interface HybridSearchOptions {
  embedder: string;
  semanticRatio: number;
}

export interface SearchOptions {
  offset?: number;
  limit?: number;
  attributesToRetrieve?: string[];
  hybrid?: HybridSearchOptions;
  retrieveVectors?: boolean;
}

export interface UpdateSettingsPayload {
  primaryKey?: string;
  searchableAttributes?: string[];
  displayedAttributes?: string[];
  filterableAttributes?: string[];
  sortableAttributes?: string[];
  embedders?: Record<string, RestEmbedderSettings | null>;
}

export interface OpenAICompatibleRestEmbedderOptions {
  url: string;
  model: string;
  apiKey?: string;
  dimensions: number;
  headers?: Record<string, string>;
  documentTemplate?: string;
}
```

The helper should produce a REST embedder config that works for OpenRouter or GLM-compatible embedding APIs:

```ts
export function openAICompatibleRestEmbedder(
  opts: OpenAICompatibleRestEmbedderOptions,
): RestEmbedderSettings {
  return {
    source: 'rest',
    url: opts.url,
    apiKey: opts.apiKey,
    dimensions: opts.dimensions,
    headers: opts.headers,
    documentTemplate: opts.documentTemplate,
    request: {
      model: opts.model,
      input: ['{{text}}', '{{..}}'],
    },
    response: {
      data: [{ embedding: '{{embedding}}' }, '{{..}}'],
    },
  };
}
```

### Native boundary shape for this phase

Do **not** send the full `embedders` map through generated N-API types. Keep the native surface smaller:

```ts
export interface IndexSettingsUpdate {
  primaryKey?: string;
  searchableAttributes?: string[];
  displayedAttributes?: string[];
  filterableAttributes?: string[];
  sortableAttributes?: string[];
  embeddersJson?: string;
}

export interface SearchOptions {
  offset?: number;
  limit?: number;
  attributesToRetrieve?: string[];
  hybridEmbedder?: string;
  hybridSemanticRatio?: number;
  retrieveVectors?: boolean;
}
```

That lets the public SDK stay ergonomic while the native boundary stays easy to generate and maintain.

### Task 1: Lock the phase 1 user-facing behavior with failing integration tests

**Files:**
- Create: `packages/api/__test__/helpers/openai-compatible-embedder.ts`
- Modify: `packages/api/__test__/engine.test.ts`
- Test: `packages/api/__test__/engine.test.ts`

- [ ] **Step 1: Write the failing embedder test helper**

Create `packages/api/__test__/helpers/openai-compatible-embedder.ts` with a deterministic local server:

```ts
import { createServer, type Server } from 'node:http';
import assert from 'node:assert/strict';

export interface FakeEmbedderServer {
  url: string;
  close(): Promise<void>;
}

function vectorFor(text: string): number[] {
  const lower = text.toLowerCase();
  const space = Number(lower.includes('space') || lower.includes('galaxy') || lower.includes('orbit'));
  const cooking = Number(lower.includes('recipe') || lower.includes('cooking') || lower.includes('kitchen'));
  const finance = Number(lower.includes('finance') || lower.includes('market') || lower.includes('stock'));
  return [space, cooking, finance];
}

export async function startOpenAICompatibleEmbedder(): Promise<FakeEmbedderServer> {
  const server: Server = createServer(async (req, res) => {
    assert.equal(req.method, 'POST');
    assert.equal(req.url, '/v1/embeddings');

    let body = '';
    for await (const chunk of req) {
      body += chunk;
    }

    const parsed = JSON.parse(body) as { input: string[] | string; model: string };
    const inputs = Array.isArray(parsed.input) ? parsed.input : [parsed.input];

    res.setHeader('content-type', 'application/json');
    res.end(
      JSON.stringify({
        data: inputs.map((input, index) => ({
          object: 'embedding',
          index,
          embedding: vectorFor(input),
        })),
      }),
    );
  });

  await new Promise<void>((resolve) => server.listen(0, '127.0.0.1', () => resolve()));
  const address = server.address();
  if (!address || typeof address === 'string') {
    throw new Error('failed to bind fake embedder server');
  }

  return {
    url: `http://127.0.0.1:${address.port}/v1/embeddings`,
    close: () => new Promise((resolve, reject) => server.close((err) => (err ? reject(err) : resolve()))),
  };
}
```

- [ ] **Step 2: Write the failing integration tests**

Append these tests to `packages/api/__test__/engine.test.ts`:

```ts
import { startOpenAICompatibleEmbedder } from './helpers/openai-compatible-embedder.ts';
```

```ts
test('Index: updateSettings accepts REST embedders and hybrid search uses semantic results', async () => {
  const dir = mkdtempSync(join(tmpdir(), 'msb-'));
  const embedder = await startOpenAICompatibleEmbedder();
  try {
    const client = new Client({ dataDir: dir });
    const index = await client.createIndex('docs', { primaryKey: 'id' });

    const settingsTask = await index.updateSettings({
      embedders: {
        default: openAICompatibleRestEmbedder({
          url: embedder.url,
          model: 'text-embedding-3-small',
          apiKey: 'test-key',
          dimensions: 3,
          documentTemplate: '{{doc.title}} {{doc.overview}}',
        }),
      },
    });
    await client.waitForTask(settingsTask.taskUid);

    const addTask = await index.addDocuments([
      { id: '1', title: 'Galaxy guide', overview: 'orbital mechanics and nebula routes' },
      { id: '2', title: 'Pasta manual', overview: 'cooking fresh noodles in the kitchen' },
    ]);
    await client.waitForTask(addTask.taskUid);

    const results = await index.search('space travel', {
      hybrid: { embedder: 'default', semanticRatio: 1.0 },
    });

    assert.equal(results.hits.length, 1);
    assert.equal(results.hits[0]?.id, '1');
    assert.equal(results.hits[0]?.title, 'Galaxy guide');
  } finally {
    await embedder.close();
    rmSync(dir, { recursive: true, force: true });
  }
});

test('Index: search with retrieveVectors returns generated vectors for hits', async () => {
  const dir = mkdtempSync(join(tmpdir(), 'msb-'));
  const embedder = await startOpenAICompatibleEmbedder();
  try {
    const client = new Client({ dataDir: dir });
    const index = await client.createIndex('docs', { primaryKey: 'id' });

    await client.waitForTask(
      (
        await index.updateSettings({
          embedders: {
            default: openAICompatibleRestEmbedder({
              url: embedder.url,
              model: 'text-embedding-3-small',
              apiKey: 'test-key',
              dimensions: 3,
              documentTemplate: '{{doc.title}} {{doc.overview}}',
            }),
          },
        })
      ).taskUid,
    );

    await client.waitForTask(
      (
        await index.addDocuments([
          { id: '1', title: 'Galaxy guide', overview: 'orbital mechanics and nebula routes' },
        ])
      ).taskUid,
    );

    const results = await index.search('space', {
      hybrid: { embedder: 'default', semanticRatio: 1.0 },
      retrieveVectors: true,
    });

    assert.equal(results.hits.length, 1);
    assert.ok(results.hits[0]?._vectors);
    assert.ok(results.hits[0]?._vectors.default);
  } finally {
    await embedder.close();
    rmSync(dir, { recursive: true, force: true });
  }
});

test('openAICompatibleRestEmbedder builds a REST embedder config for OpenRouter-like providers', () => {
  const config = openAICompatibleRestEmbedder({
    url: 'https://openrouter.ai/api/v1/embeddings',
    model: 'openai/text-embedding-3-small',
    apiKey: 'token',
    dimensions: 3,
    headers: { 'HTTP-Referer': 'https://example.com' },
  });

  assert.deepEqual(config, {
    source: 'rest',
    url: 'https://openrouter.ai/api/v1/embeddings',
    apiKey: 'token',
    dimensions: 3,
    headers: { 'HTTP-Referer': 'https://example.com' },
    request: {
      model: 'openai/text-embedding-3-small',
      input: ['{{text}}', '{{..}}'],
    },
    response: {
      data: [{ embedding: '{{embedding}}' }, '{{..}}'],
    },
  });
});
```

- [ ] **Step 3: Run the tests to verify they fail**

Run:

```bash
pnpm --filter @meilisearch-bridge/api test
```

Expected:

- TypeScript compile failure for missing `embedders`, `hybrid`, `retrieveVectors`, and `openAICompatibleRestEmbedder`
- or runtime failure because native `updateSettings()` and `search()` ignore the new fields

- [ ] **Step 4: Commit the red test state**

```bash
git add packages/api/__test__/engine.test.ts packages/api/__test__/helpers/openai-compatible-embedder.ts
git commit -m "test(api): add failing rest embedder integration coverage"
```

### Task 2: Add the phase 1 TypeScript SDK surface and the OpenAI-compatible helper

**Files:**
- Modify: `packages/api/src/index.ts`
- Test: `packages/api/__test__/engine.test.ts`

- [ ] **Step 1: Add the public TypeScript types and helper**

In `packages/api/src/index.ts`, insert these types near the existing settings and search option types:

```ts
export interface RestEmbedderSettings {
  source: 'rest';
  url: string;
  apiKey?: string;
  dimensions?: number;
  documentTemplate?: string;
  documentTemplateMaxBytes?: number;
  request: Record<string, unknown>;
  response: Record<string, unknown>;
  headers?: Record<string, string>;
}

export interface HybridSearchOptions {
  embedder: string;
  semanticRatio: number;
}

export interface RetrievedVectors {
  [embedderName: string]: {
    regenerate?: boolean;
    embeddings?: number[][];
  };
}

export interface OpenAICompatibleRestEmbedderOptions {
  url: string;
  model: string;
  apiKey?: string;
  dimensions: number;
  headers?: Record<string, string>;
  documentTemplate?: string;
}
```

Add the helper below those types:

```ts
export function openAICompatibleRestEmbedder(
  opts: OpenAICompatibleRestEmbedderOptions,
): RestEmbedderSettings {
  return {
    source: 'rest',
    url: opts.url,
    apiKey: opts.apiKey,
    dimensions: opts.dimensions,
    headers: opts.headers,
    documentTemplate: opts.documentTemplate,
    request: {
      model: opts.model,
      input: ['{{text}}', '{{..}}'],
    },
    response: {
      data: [{ embedding: '{{embedding}}' }, '{{..}}'],
    },
  };
}
```

- [ ] **Step 2: Extend the public settings and search payloads**

Replace the existing payload definitions with these versions:

```ts
export interface UpdateSettingsPayload {
  primaryKey?: string;
  searchableAttributes?: string[];
  displayedAttributes?: string[];
  filterableAttributes?: string[];
  sortableAttributes?: string[];
  embedders?: Record<string, RestEmbedderSettings | null>;
}

export interface SearchOptions {
  offset?: number;
  limit?: number;
  attributesToRetrieve?: string[];
  hybrid?: HybridSearchOptions;
  retrieveVectors?: boolean;
}
```

- [ ] **Step 3: Translate the public payloads into the native DTOs**

Add these helper functions in `packages/api/src/index.ts`:

```ts
function toNativeSettings(settings: UpdateSettingsPayload): NativeIndexSettingsUpdate {
  return {
    primaryKey: settings.primaryKey,
    searchableAttributes: settings.searchableAttributes,
    displayedAttributes: settings.displayedAttributes,
    filterableAttributes: settings.filterableAttributes,
    sortableAttributes: settings.sortableAttributes,
    embeddersJson: settings.embedders ? JSON.stringify(settings.embedders) : undefined,
  };
}

function toNativeSearchOptions(opts?: SearchOptions): NativeSearchOptions {
  return {
    offset: opts?.offset,
    limit: opts?.limit,
    attributesToRetrieve: opts?.attributesToRetrieve,
    hybridEmbedder: opts?.hybrid?.embedder,
    hybridSemanticRatio: opts?.hybrid?.semanticRatio,
    retrieveVectors: opts?.retrieveVectors,
  };
}
```

Use them in the existing methods:

```ts
const task = await this.#native.updateSettings(toNativeSettings(settings));
```

```ts
const results = await this.#native.search(query, toNativeSearchOptions(opts));
```

- [ ] **Step 4: Run the tests to verify TypeScript passes but runtime still fails**

Run:

```bash
pnpm --filter @meilisearch-bridge/api test
```

Expected:

- TypeScript passes
- the new integration tests still fail because `packages/core` does not yet apply embedders, hybrid search, or vector retrieval

- [ ] **Step 5: Commit the SDK surface**

```bash
git add packages/api/src/index.ts packages/api/__test__/engine.test.ts packages/api/__test__/helpers/openai-compatible-embedder.ts
git commit -m "feat(api): add rest embedder and hybrid search sdk types"
```

### Task 3: Add native embedder parsing and runtime construction in `packages/core`

**Files:**
- Create: `packages/core/src/embedders.rs`
- Modify: `packages/core/src/lib.rs`
- Modify: `packages/core/src/index.rs`
- Test: `packages/api/__test__/engine.test.ts`

- [ ] **Step 1: Create a dedicated embedder helper module**

Create `packages/core/src/embedders.rs` with the JSON parsing and runtime builder:

```rust
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use milli::prompt::Prompt;
use milli::update::Setting;
use milli::vector::embedder;
use milli::vector::settings::{EmbedderSource, EmbeddingSettings};
use milli::vector::{RuntimeEmbedder, RuntimeEmbedders};
use serde::Deserialize;

use crate::errors::{BridgeError, BridgeErrorCode, BridgeResult};

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RestEmbedderInput {
    pub source: String,
    pub url: String,
    pub api_key: Option<String>,
    pub dimensions: Option<usize>,
    pub document_template: Option<String>,
    pub document_template_max_bytes: Option<usize>,
    pub request: serde_json::Value,
    pub response: serde_json::Value,
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
}

pub fn parse_embedder_settings(
    embedders_json: &str,
) -> BridgeResult<BTreeMap<String, Setting<EmbeddingSettings>>> {
    let parsed: BTreeMap<String, Option<RestEmbedderInput>> =
        serde_json::from_str(embedders_json).map_err(|err| BridgeError {
            code: BridgeErrorCode::InvalidArgument,
            message: format!("invalid embedders JSON: {err}"),
        })?;

    parsed
        .into_iter()
        .map(|(name, value)| match value {
            Some(input) => {
                if input.source != "rest" {
                    return Err(BridgeError {
                        code: BridgeErrorCode::InvalidArgument,
                        message: format!("embedder `{name}` must use source `rest` in this phase"),
                    });
                }

                Ok((
                    name,
                    Setting::Set(EmbeddingSettings {
                        source: Setting::Set(EmbedderSource::Rest),
                        url: Setting::Set(input.url),
                        api_key: Setting::Set(input.api_key.unwrap_or_default()),
                        dimensions: input.dimensions.map_or(Setting::NotSet, Setting::Set),
                        document_template: input
                            .document_template
                            .map_or(Setting::NotSet, Setting::Set),
                        document_template_max_bytes: input
                            .document_template_max_bytes
                            .map_or(Setting::NotSet, Setting::Set),
                        request: Setting::Set(input.request),
                        response: Setting::Set(input.response),
                        headers: Setting::Set(input.headers),
                        ..EmbeddingSettings::default()
                    }),
                ))
            }
            None => Ok((name, Setting::Reset)),
        })
        .collect()
}

pub fn build_runtime_embedders(
    index: &milli::Index,
    rtxn: &milli::heed::RoTxn<'_>,
    ip_policy: &http_client::policy::IpPolicy,
) -> BridgeResult<RuntimeEmbedders> {
    let embedding_configs = index.embedding_configs();
    let mut runtime = HashMap::new();

    for config in embedding_configs.embedding_configs(rtxn).map_err(BridgeError::from)? {
        let info = embedding_configs
            .embedder_info(rtxn, &config.name)
            .map_err(BridgeError::from)?
            .ok_or_else(|| BridgeError {
                code: BridgeErrorCode::InvalidDatabaseState,
                message: format!("missing embedder info for `{}`", config.name),
            })?;

        let embedder = embedder::Embedder::new(config.config.embedder_options.clone(), 32, *ip_policy)
            .map_err(|err| BridgeError {
                code: BridgeErrorCode::SettingsUpdateInvalid,
                message: err.to_string(),
            })?;

        let prompt = Prompt::new(
            config.config.prompt.template.clone(),
            config.config.prompt.max_bytes,
        )
        .map_err(|err| BridgeError {
            code: BridgeErrorCode::SettingsUpdateInvalid,
            message: err.to_string(),
        })?;

        runtime.insert(
            config.name.clone(),
            Arc::new(RuntimeEmbedder::new(
                Arc::new(embedder),
                prompt,
                vec![],
                info.quantized(),
            )),
        );
    }

    Ok(RuntimeEmbedders::new(runtime))
}
```

- [ ] **Step 2: Export the new module**

Update `packages/core/src/lib.rs`:

```rust
pub mod embedders;
```

- [ ] **Step 3: Extend the native DTOs**

In `packages/core/src/index.rs`, extend the N-API DTOs:

```rust
#[napi(object)]
#[derive(Clone, Default)]
pub struct IndexSettingsUpdate {
    pub primary_key: Option<String>,
    pub searchable_attributes: Option<Vec<String>>,
    pub displayed_attributes: Option<Vec<String>>,
    pub filterable_attributes: Option<Vec<String>>,
    pub sortable_attributes: Option<Vec<String>>,
    pub embedders_json: Option<String>,
}

#[napi(object)]
#[derive(Clone, Default)]
pub struct SearchOptions {
    pub offset: Option<u32>,
    pub limit: Option<u32>,
    pub attributes_to_retrieve: Option<Vec<String>>,
    pub hybrid_embedder: Option<String>,
    pub hybrid_semantic_ratio: Option<f32>,
    pub retrieve_vectors: Option<bool>,
}
```

- [ ] **Step 4: Run the tests to verify the new module compiles but behavior still fails**

Run:

```bash
pnpm run build:core
pnpm --filter @meilisearch-bridge/api test
```

Expected:

- native build succeeds
- hybrid search and retrieveVectors tests still fail because settings application and search execution are not yet wired

- [ ] **Step 5: Commit the native parsing layer**

```bash
git add packages/core/src/lib.rs packages/core/src/index.rs packages/core/src/embedders.rs
git commit -m "feat(core): add rest embedder parsing helpers"
```

### Task 4: Apply phase 1 embedder settings and wire runtime embedders into indexing, hybrid search, and `retrieveVectors`

**Files:**
- Modify: `packages/core/src/index.rs`
- Modify: `packages/core/src/embedders.rs`
- Test: `packages/api/__test__/engine.test.ts`

- [ ] **Step 1: Apply embedder settings through `milli::update::Settings`**

In `process_settings_update`, add embedder application:

```rust
use crate::embedders::{build_runtime_embedders, parse_embedder_settings};
```

```rust
if let Some(embedders_json) = settings.embedders_json.as_deref() {
    let embedders = parse_embedder_settings(embedders_json)?;
    update.set_embedder_settings(embedders);
}
```

Keep the rest of the existing settings behavior unchanged.

- [ ] **Step 2: Pass runtime embedders into document indexing**

In `process_document_addition`, build runtime embedders before creating `IndexDocuments` and pass them in:

```rust
let runtime_embedders = {
    let rtxn = idx.read_txn().map_err(BridgeError::from)?;
    build_runtime_embedders(&idx, &rtxn, &ip_policy)?
};

let indexer = milli::update::IndexDocuments::new(
    &mut wtxn,
    &idx,
    &indexer_config,
    milli::update::IndexDocumentsConfig::default(),
    |_| {},
    &must_stop,
    &embedder_stats,
    &ip_policy,
)?
.with_embedders(runtime_embedders);
```

- [ ] **Step 3: Wire minimal hybrid search**

In `Index::search`, when both `hybrid_embedder` and `hybrid_semantic_ratio` are present:

```rust
let runtime_embedders = {
    let ip_policy = http_client::policy::IpPolicy::danger_always_allow();
    build_runtime_embedders(&idx, &rtxn, &ip_policy)?
};

if let (Some(embedder_name), Some(semantic_ratio)) = (
    options.hybrid_embedder.clone(),
    options.hybrid_semantic_ratio,
) {
    let runtime = runtime_embedders.get(&embedder_name).ok_or_else(|| BridgeError {
        code: BridgeErrorCode::SearchError,
        message: format!("unknown hybrid embedder `{embedder_name}`"),
    })?;

    search.semantic(
        embedder_name,
        runtime.embedder.clone(),
        runtime.is_quantized,
        None,
        None,
    );

    let (executed, _semantic_hit_count) = search
        .execute_hybrid(semantic_ratio)
        .map_err(|err| BridgeError {
            code: BridgeErrorCode::SearchError,
            message: err.to_string(),
        })?;

    // keep the existing hit materialization logic, but feed it `executed`
}
```

If the hybrid fields are absent, keep the existing `search.execute()` path.

- [ ] **Step 4: Expose generated vectors when requested**

In `Index::search`, set the retrieve-vectors flag before executing the query:

```rust
search.retrieve_vectors(options.retrieve_vectors.unwrap_or(false));
```

When materializing hits, preserve `_vectors` in `attributes` so the API layer can surface it unchanged. Do not strip it in the first phase.

- [ ] **Step 5: Run the focused verification**

Run:

```bash
pnpm run build:core
pnpm --filter @meilisearch-bridge/api test
```

Expected:

- the new REST embedder + hybrid search tests pass
- the existing document and settings tests remain green

- [ ] **Step 6: Commit the runtime wiring**

```bash
git add packages/core/src/index.rs packages/core/src/embedders.rs packages/api/__test__/engine.test.ts
git commit -m "feat(core): wire rest embedders into indexing hybrid search and vector retrieval"
```

### Task 5: Regenerate committed bindings and run the full phase 1 verification pass

**Files:**
- Modify: `packages/core/index.d.ts`
- Modify: `packages/core/index.js`
- Modify: `packages/core/Cargo.lock`
- Test: `packages/api/__test__/engine.test.ts`

- [ ] **Step 1: Regenerate the committed native bindings**

Run:

```bash
pnpm run build:core
```

Expected:

- `packages/core/index.d.ts` contains `embeddersJson`, `hybridEmbedder`, `hybridSemanticRatio`, and `retrieveVectors`
- `packages/core/index.js` remains generated and non-empty

- [ ] **Step 2: Run the complete local verification chain**

Run:

```bash
pnpm --filter @meilisearch-bridge/api lint
pnpm --filter @meilisearch-bridge/api build
pnpm --filter @meilisearch-bridge/api test
```

Expected:

- all three commands exit `0`

- [ ] **Step 3: Run the repository API verification script**

Run:

```bash
pnpm run verify:api
```

Expected:

- the script exits `0`

- [ ] **Step 4: Commit generated bindings and lockfile changes**

```bash
git add packages/core/index.d.ts packages/core/index.js packages/core/Cargo.lock packages/core/src/embedders.rs packages/core/src/index.rs packages/core/src/lib.rs packages/api/src/index.ts packages/api/__test__/engine.test.ts packages/api/__test__/helpers/openai-compatible-embedder.ts
git commit -m "feat: add rest embedder support for openrouter and glm"
```

### Task 6: Phase 2 follow-up for `similar`

**Files:**
- Modify: `packages/api/src/index.ts`
- Modify: `packages/core/src/index.rs`
- Modify: `packages/core/index.d.ts`
- Modify: `packages/core/index.js`
- Modify: `packages/api/__test__/engine.test.ts`

- [ ] **Step 1: Write the failing phase 2 tests**

Add tests for a new `index.similar(documentId, opts)` entry point:

```ts
test('Index: similar returns semantically related documents for a reference document id', async () => {
  const dir = mkdtempSync(join(tmpdir(), 'msb-'));
  const embedder = await startOpenAICompatibleEmbedder();
  try {
    const client = new Client({ dataDir: dir });
    const index = await client.createIndex('docs', { primaryKey: 'id' });

    await client.waitForTask(
      (
        await index.updateSettings({
          embedders: {
            default: openAICompatibleRestEmbedder({
              url: embedder.url,
              model: 'text-embedding-3-small',
              dimensions: 3,
            }),
          },
        })
      ).taskUid,
    );

    await client.waitForTask(
      (
        await index.addDocuments([
          { id: '1', title: 'Galaxy guide', overview: 'orbital mechanics and nebula routes' },
          { id: '2', title: 'Star atlas', overview: 'space maps and orbits' },
          { id: '3', title: 'Pasta manual', overview: 'cooking fresh noodles in the kitchen' },
        ])
      ).taskUid,
    );

    const results = await index.similar('1', { embedder: 'default', limit: 1 });
    assert.equal(results.hits.length, 1);
    assert.equal(results.hits[0]?.id, '2');
  } finally {
    await embedder.close();
    rmSync(dir, { recursive: true, force: true });
  }
});
```

- [ ] **Step 2: Run the tests to verify they fail**

Run:

```bash
pnpm --filter @meilisearch-bridge/api test
```

Expected:

- failure because `similar` is not yet implemented in `api` or `core`

- [ ] **Step 3: Add the minimal public and native API**

Expose these phase 2 shapes:

```ts
export interface SimilarOptions {
  embedder: string;
  offset?: number;
  limit?: number;
  filter?: string;
  retrieveVectors?: boolean;
}
```

Add a matching native DTO plus an `Index.similar(...)` N-API method.

- [ ] **Step 4: Implement `milli::Similar`-based execution**

In `packages/core/src/index.rs`, build runtime embedders exactly as in phase 1, then call `milli`'s similar-search path using:

- the requested document id
- the embedder name
- `retrieveVectors`
- limit and offset

Keep the result materialization aligned with `search()`.

- [ ] **Step 5: Run the tests to verify phase 2 passes**

Run:

```bash
pnpm run build:core
pnpm --filter @meilisearch-bridge/api test
```

Expected:

- the new `similar` tests pass
- phase 1 tests remain green

- [ ] **Step 6: Regenerate bindings and commit**

Run:

```bash
pnpm run build:core
```

Commit:

```bash
git add packages/core/src/index.rs packages/core/index.d.ts packages/core/index.js packages/api/src/index.ts packages/api/__test__/engine.test.ts
git commit -m "feat: add similar search for embedded documents"
```

## Self-review

- Spec coverage:
  - phase 1 public `embedders` settings: covered by Tasks 1, 2, 4
  - OpenRouter/GLM-friendly helper: covered by Tasks 1 and 2
  - runtime embedding during indexing: covered by Task 4
  - minimal hybrid search: covered by Task 4
  - `retrieveVectors` in the base search shape: covered by Tasks 1, 2, 4
  - generated binding refresh and full phase 1 verification: covered by Task 5
  - phase 2 `similar`: covered by Task 6
- Placeholder scan:
  - no `TODO`, `TBD`, or “similar to previous task” instructions remain
- Type consistency:
  - public TS uses `embedders` and `hybrid`
  - phase 1 public TS also uses `retrieveVectors`
  - native TS DTOs use `embeddersJson`, `hybridEmbedder`, `hybridSemanticRatio`, and `retrieveVectors`
  - Rust uses matching snake_case fields generated from those names
