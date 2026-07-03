# AGENTS.md

This file is a quick onboarding note for humans and AI agents working in this repository.

## Project goal

Expose Meilisearch’s search engine as a Node.js addon (napi-rs) and ship a TypeScript SDK whose developer experience and API shape closely follow the official `meilisearch-js` SDK, but without an HTTP server process.

## High-level architecture

- `packages/core/` — Rust + napi-rs native addon
  - Wraps vendored `milli` (Meilisearch search engine) and exposes `Engine`, `Index`, tasks, and result types to Node.js.
- `packages/api/` — TypeScript SDK
  - A higher-level SDK that aims to mirror `meilisearch-js`’s ergonomics while calling into `@meilisearch-bridge/core`.
- `native/meilisearch/` — Vendored upstream (git subtree)
  - Read-only copy of Meilisearch pinned at a tag. This repo does not modify vendored upstream code.

## Repository constraints

- Do not modify `native/meilisearch/` (vendored upstream). Upgrade only via `git subtree pull`.
- Keep the public API stable and test-driven: changes in `packages/core` must be reflected in `packages/api` and covered by tests.
- Prefer aligning with upstream conventions (Meilisearch + napi-rs) over inventing new shapes.

## Common commands

Install dependencies:

```bash
pnpm install
```

Local “API milestone” verification (lint + build + test, with correct core → api order):

```bash
pnpm run verify:api
```

CI-style verification (assumes `@meilisearch-bridge/core` build artifacts are already present, e.g. downloaded in CI):

```bash
pnpm run verify:api:ci
```

Build core native addon:

```bash
pnpm run build:core
```

## CI overview

- `.github/workflows/lint.yml`
  - Runs `cargo fmt --check` and `tsc --noEmit`.
- `.github/workflows/test-suite.yml`
  - Builds the native addon, uploads build artifacts, then runs the TypeScript build/test job using those artifacts.

## Release / publishing (planned)

The repository currently lacks a full “build matrix + publish to npm” pipeline.
The next step is to add a publish workflow aligned with napi-rs official templates:

- build per target triple (linux/macos/windows)
- run `napi prepublish` and publish to npm using `NPM_TOKEN`
- attach artifacts / provenance as appropriate

## Where to start reading code

- Core public API entry points: `packages/core/src/engine.rs`, `packages/core/src/index.rs`
- Task model & persistence: `packages/core/src/task.rs`
- SDK wrapper and error normalization: `packages/api/src/index.ts`
