/**
 * @yuyi919/meilisearch-bridge — high-level SDK mirroring `meilisearch-js` shape.
 *
 * Design rule: every public method returns a Promise (matching meilisearch-js).
 * Native exceptions from `@yuyi919/meilisearch-bridge-core` are normalized into
 * `MeilisearchBridgeError` with a stable `code` field so callers can switch
 * on the same string codes that meilisearch-js uses (`index_not_found`, etc.).
 *
 * NOTE: this is the in-process SDK. It does NOT speak HTTP. There is no
 * `Client(url, apiKey)` constructor — you instantiate an `Engine` against
 * a local directory and then build a `Client` from it.
 */

import {
  Engine as NativeEngine,
  Index as NativeIndex,
  type GetDocumentsOptions as NativeGetDocumentsOptions,
  type IndexSettingsUpdate as NativeIndexSettingsUpdate,
  type SearchOptions as NativeSearchOptions,
  type TaskInfo as NativeTaskInfo,
} from "@yuyi919/meilisearch-bridge-core";

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

export interface UpdateSettingsPayload {
  primaryKey?: string;
  searchableAttributes?: string[];
  displayedAttributes?: string[];
  filterableAttributes?: string[];
  sortableAttributes?: string[];
}

export interface GetDocumentsOptions {
  offset?: number;
  limit?: number;
  fields?: string[];
}

export interface SearchOptions {
  offset?: number;
  limit?: number;
  attributesToRetrieve?: string[];
}

export interface TaskDetails {
  receivedDocuments?: number;
  indexedDocuments?: number;
  searchableAttributes?: string[];
}

export interface Task {
  uid: number;
  indexUid?: string;
  status: string;
  type: string;
  details?: TaskDetails;
  error?: string;
  enqueuedAt: string;
  startedAt?: string;
  finishedAt?: string;
}

export interface EnqueuedTask {
  taskUid: number;
  indexUid?: string;
  status: string;
  type: string;
  enqueuedAt: string;
}

/** Stable error codes surfaced from the native layer. */
export type MeilisearchErrorCode =
  | "Internal"
  | "InvalidArgument"
  | "IoError"
  | "IndexAlreadyExists"
  | "IndexNotFound"
  | "TaskNotFound"
  | "DocumentNotFound"
  | "InvalidDatabaseState"
  | "SettingsUpdateInvalid"
  | "SearchError"
  | "TooManyDocuments"
  | "OutOfBound"
  | "Disposed";

/** Error thrown by this SDK. Mirrors `MeilisearchApiError` from meilisearch-js. */
export class MeilisearchBridgeError extends Error {
  readonly code: MeilisearchErrorCode;
  readonly cause?: unknown;
  constructor(code: MeilisearchErrorCode, message: string, cause?: unknown) {
    super(message);
    this.name = "MeilisearchBridgeError";
    this.code = code;
    this.cause = cause;
  }
}

function normalizeNativeError(e: unknown): never {
  if (e && typeof e === "object" && "code" in e && "message" in e) {
    const message = String((e as { message: unknown }).message);
    const rawCode = String((e as { code: unknown }).code);
    throw new MeilisearchBridgeError(
      extractErrorCode(rawCode, message),
      message,
      e,
    );
  }
  throw new MeilisearchBridgeError("Internal", String(e), e);
}

const KNOWN_ERROR_CODES = new Set<MeilisearchErrorCode>([
  "Internal",
  "InvalidArgument",
  "IoError",
  "IndexAlreadyExists",
  "IndexNotFound",
  "TaskNotFound",
  "DocumentNotFound",
  "InvalidDatabaseState",
  "SettingsUpdateInvalid",
  "SearchError",
  "TooManyDocuments",
  "OutOfBound",
  "Disposed",
]);

function extractErrorCode(
  rawCode: string,
  message: string,
): MeilisearchErrorCode {
  if (KNOWN_ERROR_CODES.has(rawCode as MeilisearchErrorCode)) {
    return rawCode as MeilisearchErrorCode;
  }

  const matched = message.match(
    /\b(Internal|InvalidArgument|IoError|IndexAlreadyExists|IndexNotFound|TaskNotFound|DocumentNotFound|InvalidDatabaseState|SettingsUpdateInvalid|SearchError|TooManyDocuments|OutOfBound|Disposed)\b/,
  );
  if (matched) {
    return matched[1] as MeilisearchErrorCode;
  }

  return "Internal";
}

function toTask(task: NativeTaskInfo): Task {
  return {
    uid: task.uid,
    indexUid: task.indexUid ?? undefined,
    status: task.status,
    type: task.type,
    details: task.details ?? undefined,
    error: task.error ?? undefined,
    enqueuedAt: task.enqueuedAt,
    startedAt: task.startedAt ?? undefined,
    finishedAt: task.finishedAt ?? undefined,
  };
}

function toEnqueuedTask(task: NativeTaskInfo): EnqueuedTask {
  return {
    taskUid: task.uid,
    indexUid: task.indexUid ?? undefined,
    status: task.status,
    type: task.type,
    enqueuedAt: task.enqueuedAt,
  };
}

/**
 * The `Engine` is the in-process equivalent of a running `meilisearch`
 * server. It manages a directory of indexes on disk.
 */
export class Engine {
  #native: NativeEngine;
  readonly dataDir: string;

  constructor(opts: EngineOptions) {
    this.dataDir = opts.dataDir;
    try {
      this.#native = new NativeEngine(opts.dataDir);
    } catch (e) {
      normalizeNativeError(e);
    }
  }

  /** Release native resources held by this engine and prevent further use.
   *
   * After this returns, any method call throws `MeilisearchBridgeError` with
   * code `Disposed`. Outstanding `Index` handles are unaffected — dispose
   * them individually. Idempotent.
   *
   * The native handle is intentionally NOT nulled: the native struct owns the
   * disposed flag and is the single source of truth for the `Disposed` error
   * surfaced to callers. */
  dispose(): void {
    this.#native.dispose();
  }

  /** Explicit Resource Management hook for `using engine = new Engine(...)`. */
  [Symbol.dispose](): void {
    this.dispose();
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
      return new Index(native, native.uid, native.primaryKey ?? null);
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

  async getTask(taskUid: number): Promise<Task> {
    try {
      return toTask(await this.#native.getTask(taskUid));
    } catch (e) {
      normalizeNativeError(e);
    }
  }

  async waitForTask(taskUid: number, timeoutMs?: number): Promise<Task> {
    try {
      return toTask(await this.#native.waitForTask(taskUid, timeoutMs ?? null));
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
  #native: NativeIndex;
  readonly uid: string;
  primaryKey: string | null;

  constructor(native: NativeIndex, uid: string, primaryKey: string | null) {
    this.#native = native;
    this.uid = uid;
    this.primaryKey = primaryKey;
  }

  /** Release native resources held by this index handle and prevent further
   * use.
   *
   * Only disables this handle — sibling handles backed by the same index
   * keep working. In-flight background tasks (add/update) run to completion.
   * Idempotent.
   *
   * The native handle is intentionally NOT nulled: the native struct owns the
   * disposed flag and is the single source of truth for the `Disposed` error. */
  dispose(): void {
    this.#native.dispose();
  }

  /** Explicit Resource Management hook for `using index = ...`. */
  [Symbol.dispose](): void {
    this.dispose();
  }

  /** Total number of documents stored in the index. */
  async getDocuments<
    T extends Record<string, unknown> = Record<string, unknown>,
  >(
    opts?: GetDocumentsOptions,
  ): Promise<{ results: T[]; offset: number; limit: number; total: number }> {
    try {
      return (await this.#native.getDocuments(
        opts as NativeGetDocumentsOptions | undefined,
      )) as { results: T[]; offset: number; limit: number; total: number };
    } catch (e) {
      normalizeNativeError(e);
    }
  }

  /**
   * Add or replace documents in the index. Mirrors
   * `meilisearch-js`'s `index.addDocuments()` signature.
   */
  async addDocuments<T extends Record<string, unknown>>(
    documents: T[],
    opts?: AddDocumentsOptions,
  ): Promise<EnqueuedTask> {
    if (opts?.primaryKey && !this.primaryKey) {
      await this.updateSettings({ primaryKey: opts.primaryKey });
    }
    const ndjson = documents.map((d) => JSON.stringify(d)).join("\n");
    try {
      return toEnqueuedTask(await this.#native.addDocumentsFromNdjson(ndjson));
    } catch (e) {
      normalizeNativeError(e);
    }
  }

  async updateSettings(settings: UpdateSettingsPayload): Promise<EnqueuedTask> {
    try {
      const task = await this.#native.updateSettings(
        settings as NativeIndexSettingsUpdate,
      );
      if (settings.primaryKey) {
        this.primaryKey = settings.primaryKey;
      }
      return toEnqueuedTask(task);
    } catch (e) {
      normalizeNativeError(e);
    }
  }

  async search<T extends Record<string, any>>(
    query: string,
    opts?: SearchOptions,
  ): Promise<{
    hits: Array<T & { id: string; _rankingScore: number }>;
    estimatedTotalHits: number;
    processingTimeMs: number;
    query: string;
    isEmptyQuery: boolean;
  }> {
    try {
      const results = await this.#native.search(query, {
        offset: opts?.offset,
        limit: opts?.limit,
        attributesToRetrieve: opts?.attributesToRetrieve,
      } as NativeSearchOptions);
      return {
        hits: results.hits.map((hit) => ({
          ...(hit.attributes as T),
          id: hit.id,
          _rankingScore: hit.score,
        })),
        estimatedTotalHits: results.estimatedTotalHits,
        processingTimeMs: results.processingTimeMs,
        query: results.query,
        isEmptyQuery: results.isEmptyQuery,
      };
    } catch (e) {
      normalizeNativeError(e);
    }
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

  async getTask(taskUid: number): Promise<Task> {
    return this.engine.getTask(taskUid);
  }

  async waitForTask(taskUid: number, timeoutMs?: number): Promise<Task> {
    return this.engine.waitForTask(taskUid, timeoutMs);
  }

  /** Alias of `engine.deleteIndex()`. */
  async deleteIndex(uid: string): Promise<void> {
    return this.engine.deleteIndex(uid);
  }

  /** Release native resources held by this client handle and prevent further use. */
  dispose() {
    this.engine.dispose()
  }
}

export { Client as default, Client as MeilisearchBridge };
