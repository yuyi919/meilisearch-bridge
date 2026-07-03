export interface TaskDetails {
  receivedDocuments?: number;
  indexedDocuments?: number;
  searchableAttributes?: string[];
}

export interface TaskInfo {
  uid: number;
  indexUid?: string | null;
  status: string;
  type: string;
  details?: TaskDetails | null;
  error?: string | null;
  enqueuedAt: string;
  startedAt?: string | null;
  finishedAt?: string | null;
}

export interface IndexSettingsUpdate {
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

export interface GetDocumentsResults {
  results: Array<Record<string, unknown>>;
  offset: number;
  limit: number;
  total: number;
}

export interface SearchOptions {
  offset?: number;
  limit?: number;
  attributesToRetrieve?: string[];
}

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
  getTask(taskUid: number): Promise<TaskInfo>;
  waitForTask(taskUid: number, timeoutMs?: number | null): Promise<TaskInfo>;
}

export declare class Index {
  get uid(): string;
  get primaryKey(): string | null;
  documentCount(): number;
  getDocuments(options?: GetDocumentsOptions): Promise<GetDocumentsResults>;
  addDocumentsFromNdjson(ndjson: string): Promise<TaskInfo>;
  updateSettings(settings: IndexSettingsUpdate): Promise<TaskInfo>;
  search(query: string, options?: SearchOptions): Promise<SearchResults>;
}
