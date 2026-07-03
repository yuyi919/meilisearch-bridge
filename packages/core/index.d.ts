export declare const VERSION: string;
export declare const VENDORED_MEILISEARCH_VERSION: string;

export interface DocumentHit {
  id: string;
  score: number;
  attributes: Record<string, unknown>;
}

export interface SearchResults {
  hits: DocumentHit[];
  estimatedTotalHits: number;
  processingTimeMs: number;
  query: string;
  isEmptyQuery: boolean;
}

export declare class Engine {
  constructor(basePath: string);
  listIndexes(): Promise<string[]>;
  createIndex(uid: string, primaryKey: string): Promise<void>;
  getIndex(uid: string, primaryKey?: string | null): Promise<Index>;
  deleteIndex(uid: string): Promise<void>;
}

export declare class Index {
  get uid(): string;
  get primaryKey(): string | null;
  documentCount(): number;
  addDocumentsFromNdjson(ndjson: string): Promise<number>;
  search(query: string): Promise<SearchResults>;
}
