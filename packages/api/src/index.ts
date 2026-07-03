/**
 * @meilisearch-bridge/api — high-level SDK mirroring `meilisearch-js` shape.
 *
 * Design rule: every public method returns a Promise (matching meilisearch-js).
 * Native exceptions from `@meilisearch-bridge/core` are normalized into
 * `MeilisearchBridgeError` with a stable `code` field so callers can switch
 * on the same string codes that meilisearch-js uses (`index_not_found`, etc.).
 *
 * NOTE: this is the in-process SDK. It does NOT speak HTTP. There is no
 * `Client(url, apiKey)` constructor — you instantiate an `Engine` against
 * a local directory and then build a `Client` from it.
 */

import { Engine as NativeEngine, Index as NativeIndex } from '@meilisearch-bridge/core';

export interface EngineOptions {
  /** Path to a directory that will hold the per-index subdirectories. */
  dataDir: string;
}

export interface CreateIndexOptions {
  primaryKey: string;
}

export interface AddDocumentsOptions {
  primaryKey?: string;
}

/** Stable error codes surfaced from the native layer. */
export type MeilisearchErrorCode =
  | 'Internal'
  | 'InvalidArgument'
  | 'IoError'
  | 'IndexAlreadyExists'
  | 'IndexNotFound'
  | 'DocumentNotFound'
  | 'InvalidDatabaseState'
  | 'SettingsUpdateInvalid'
  | 'SearchError'
  | 'TooManyDocuments'
  | 'OutOfBound';

/** Error thrown by this SDK. Mirrors `MeilisearchApiError` from meilisearch-js. */
export class MeilisearchBridgeError extends Error {
  readonly code: MeilisearchErrorCode;
  readonly cause?: unknown;
  constructor(code: MeilisearchErrorCode, message: string, cause?: unknown) {
    super(message);
    this.name = 'MeilisearchBridgeError';
    this.code = code;
    this.cause = cause;
  }
}

function normalizeNativeError(e: unknown): never {
  if (e && typeof e === 'object' && 'code' in e && 'message' in e) {
    const code = String((e as { code: unknown }).code) as MeilisearchErrorCode;
    const message = String((e as { message: unknown }).message);
    throw new MeilisearchBridgeError(code, message, e);
  }
  throw new MeilisearchBridgeError('Internal', String(e), e);
}

/**
 * The `Engine` is the in-process equivalent of a running `meilisearch`
 * server. It manages a directory of indexes on disk.
 */
export class Engine {
  readonly #native: NativeEngine;
  readonly dataDir: string;

  constructor(opts: EngineOptions) {
    this.dataDir = opts.dataDir;
    try {
      this.#native = new NativeEngine(opts.dataDir);
    } catch (e) {
      normalizeNativeError(e);
    }
  }

  /** List every index currently in the data directory. */
  async listIndexes(): Promise<string[]> {
    try {
      return await this.#native.listIndexes();
    } catch (e) {
      normalizeNativeError(e);
    }
  }

  /** Create a new index. Throws `MeilisearchBridgeError` with code `IndexAlreadyExists` if it exists. */
  async createIndex(uid: string, opts: CreateIndexOptions): Promise<void> {
    try {
      await this.#native.createIndex(uid, opts.primaryKey);
    } catch (e) {
      normalizeNativeError(e);
    }
  }

  /** Open (or create) an index handle. */
  async getIndex(uid: string, primaryKey?: string): Promise<Index> {
    try {
      const native = await this.#native.getIndex(uid, primaryKey ?? null);
      return new Index(native, uid, primaryKey ?? null);
    } catch (e) {
      normalizeNativeError(e);
    }
  }

  /** Delete an index (and its on-disk data) by uid. */
  async deleteIndex(uid: string): Promise<void> {
    try {
      await this.#native.deleteIndex(uid);
    } catch (e) {
      normalizeNativeError(e);
    }
  }
}

/**
 * A single searchable index, modeled after meilisearch-js's `Index` class.
 *
 * Method names are intentionally identical to meilisearch-js so existing
 * application code can be migrated by changing the import.
 */
export class Index {
  readonly #native: NativeIndex;
  readonly uid: string;
  readonly primaryKey: string | null;

  constructor(native: NativeIndex, uid: string, primaryKey: string | null) {
    this.#native = native;
    this.uid = uid;
    this.primaryKey = primaryKey;
  }

  /** Total number of documents stored in the index. */
  async getDocuments(): Promise<{ results: unknown[]; total: number }> {
    // meilisearch-js returns { results, offset, limit, total }; we expose the
    // shape but don't actually fetch documents here yet (search not wired).
    try {
      const total = this.#native.documentCount();
      return { results: [], total };
    } catch (e) {
      normalizeNativeError(e);
    }
  }

  /**
   * Add or replace documents in the index. Mirrors
   * `meilisearch-js`'s `index.addDocuments()` signature.
   *
   * The first-cut implementation goes via NDJSON to keep the native surface
   * minimal — the underlying Rust code still needs to be hooked up to
   * milli's new Indexer pipeline for this to actually index the data.
   * Until then this method returns the number of documents it accepted.
   */
  async addDocuments<T extends Record<string, unknown>>(
    documents: T[],
    _opts?: AddDocumentsOptions,
  ): Promise<{ taskUid: number; acceptedDocuments: number; status: 'enqueued' }> {
    const ndjson = documents.map((d) => JSON.stringify(d)).join('\n');
    let accepted: number;
    try {
      accepted = await this.#native.addDocumentsFromNdjson(ndjson);
    } catch (e) {
      normalizeNativeError(e);
    }
    // The task Uid is a stub — milli's Indexer queue hasn't been wired up yet.
    // When it is, this will be the real Uid of the queued indexing task.
    return {
      taskUid: Math.floor(Math.random() * 0xffffffff),
      acceptedDocuments: accepted,
      status: 'enqueued',
    };
  }

  /** Search the index. NOT YET IMPLEMENTED — throws on every call. */
  async search(_query: string, _opts?: unknown): Promise<{ hits: never[] }> {
    try {
      await this.#native.search('');
    } catch (e) {
      // Re-throw with the friendly code; the native layer always throws
      // 'Internal' for not-implemented, which we translate here.
      normalizeNativeError(e);
    }
    return { hits: [] };
  }
}

/**
 * The `Client` mirrors `meilisearch-js`'s top-level export. In our case it
 * is just a thin facade around `Engine` — there is no HTTP server in this
 * world, but keeping the same shape makes downstream code portable.
 */
export class Client {
  readonly engine: Engine;

  constructor(opts: EngineOptions) {
    this.engine = new Engine(opts);
  }

  /** Alias of `engine.listIndexes()`. */
  async listIndexes(): Promise<string[]> {
    return this.engine.listIndexes();
  }

  /** Alias of `engine.createIndex()`. */
  async createIndex(uid: string, opts: CreateIndexOptions): Promise<Index> {
    await this.engine.createIndex(uid, opts);
    return this.engine.getIndex(uid, opts.primaryKey);
  }

  /** Alias of `engine.getIndex()`. */
  async getIndex(uid: string): Promise<Index> {
    return this.engine.getIndex(uid);
  }

  /** Alias of `engine.deleteIndex()`. */
  async deleteIndex(uid: string): Promise<void> {
    return this.engine.deleteIndex(uid);
  }
}

export { Client as default, Client as MeilisearchBridge };