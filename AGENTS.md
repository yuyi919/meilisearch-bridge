# AGENTS.md

This file is a quick onboarding note for humans and AI agents working in this repository.

## Project goal

Expose Meilisearch’s search engine as a Node.js addon (napi-rs) and ship a TypeScript SDK whose developer experience and API shape closely follow the official `meilisearch-js` SDK, but without an HTTP server process.

## High-level architecture

- `packages/core/`
  - Rust + napi-rs native addon
  - Wraps vendored `milli` and generates `index.js` plus `index.d.ts`
- `packages/api/`
  - TypeScript SDK that consumes `@yuyi919/meilisearch-bridge-core`
  - Tracks `meilisearch-js` ergonomics where implemented
- `native/meilisearch/`
  - Vendored upstream subtree
  - Read-only inside this repository

## Repository constraints

- Do not modify `native/meilisearch/` (vendored upstream). Upgrade only via `git subtree pull`.
- Keep the public API stable: changes in `packages/core` must be reflected in `packages/api` and covered by tests where behavior changes.
- Prefer aligning with upstream conventions (Meilisearch + napi-rs) over inventing new shapes.
- Treat `packages/core/index.js` and `packages/core/index.d.ts` as committed generated artifacts. If native exports change, regenerate them with `pnpm run build:core`.

## Common commands

Install dependencies:

```bash
pnpm install
```

Local “API milestone” verification (lint + build + test, with correct core → api order):

```bash
pnpm run verify:api
```

CI-style verification (assumes `@yuyi919/meilisearch-bridge-core` build artifacts are already present, e.g. downloaded in CI):

```bash
pnpm run verify:api:ci
```

Build core native addon:

```bash
pnpm run build:core
```

Build or test the SDK only:

```bash
pnpm --filter @yuyi919/meilisearch-bridge build
pnpm --filter @yuyi919/meilisearch-bridge test
```

## CI overview

- `.github/workflows/lint.yml`
  - Runs generated binding presence checks, Rust formatting, and TypeScript type-checking.
- `.github/workflows/test-suite.yml`
  - Builds the native addon, uploads generated bindings and `.node` artifacts, then runs the TypeScript build/test job using those artifacts.
- `.github/workflows/release.yml`
  - Builds target-specific binaries, verifies them, and publishes npm packages on release commits.

## Plans and specs

- Active implementation docs live under `.trae/specs/`
- Current index milestone docs:
  - `.trae/specs/implement-complete-index-sdk/spec.md`
  - `.trae/specs/implement-complete-index-sdk/tasks.md`
  - `.trae/specs/implement-complete-index-sdk/checklist.md`

## Where to start reading code

- Core public API entry points: `packages/core/src/engine.rs`, `packages/core/src/index.rs`
- Task model & persistence: `packages/core/src/task.rs`
- SDK wrapper and error normalization: `packages/api/src/index.ts`
