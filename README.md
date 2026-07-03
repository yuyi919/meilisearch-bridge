# meilisearch-bridge

> A pnpm monorepo that wraps the Meilisearch search engine (vendored as a `git subtree`) as a Node.js addon via **napi-rs**, then exposes a SDK-style TypeScript API matching the official [`meilisearch-js`](https://github.com/meilisearch/meilisearch-js) interface.

```
┌─────────────────────────────────────────────────────────────────┐
│  packages/api                                                    │
│  ┌──────────────────────────┐    ┌───────────────────────────┐   │
│  │ @meilisearch-bridge/api  │ ─► │ @meilisearch-bridge/core  │   │
│  │ (TS SDK, meilisearch-   │    │ (napi-rs native addon)    │   │
│  │  js-compatible surface)  │    │  .node + .d.ts            │   │
│  └──────────────────────────┘    └─────────────┬─────────────┘   │
└────────────────────────────────────────────────┼─────────────────┘
                                                 ▼
                              ┌──────────────────────────────────┐
                              │  native/meilisearch/             │
                              │   crates/milli (search engine)   │
                              │   crates/index-scheduler (queue) │
                              │   + other vendored crates        │
                              │  ── git subtree, NOT modified ── │
                              └──────────────────────────────────┘
```

## Why this exists

The official `meilisearch-js` SDK is a thin HTTP client for a separate `meilisearch` server process. This bridge inlines the search engine itself (via `milli` / `index-scheduler`) directly into Node.js, with no HTTP layer — useful for:

- embedding search into desktop / mobile apps (Electron, Tauri, React Native via napi)
- unit-testing search logic without spinning up a server
- serverless / edge runtimes with size constraints
- single-process tools that just want a search engine as a library

## Layout

| Path                          | What                                                     |
| ----------------------------- | -------------------------------------------------------- |
| `packages/core/`              | Rust crate wrapping milli with `#[napi]` bindings        |
| `packages/api/`               | TypeScript SDK, mirrors `meilisearch-js` API surface     |
| `native/meilisearch/`         | Vendored Meilisearch v1.48.3 (read-only, via `git subtree`) |
| `pnpm-workspace.yaml`         | Workspace root                                           |
| `.github/workflows/`          | CI mirroring upstream Meilisearch style                  |

## Status

🚧 Under construction. The first usable surface will be `Index` operations: `addDocuments`, `search`, `updateSettings`. Tasks / dump / vector search will follow.

## Vendoring policy

`native/meilisearch/` is a `git subtree` of [meilisearch/meilisearch](https://github.com/meilisearch/meilisearch) pinned at tag **`v1.48.3`**. We **do not modify its contents**. To upgrade:

```bash
git subtree pull --prefix=native/meilisearch \
  https://github.com/meilisearch/meilisearch.git <new-tag> --squash
```

> Note: the upstream repo contains an `AGENTS.md` declaring that AI agents must not engage with their forge features (issues, PRs, discussions). We respect that — this project consumes their library code only and does not open any upstream issues/PRs.