import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync } from 'node:fs';
import { rm as rmAsync } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { Client, Engine, MeilisearchBridgeError } from '../src/index';

// LMDB holds file locks briefly on Windows even after handles drop; retry
// cleanup so a slow release doesn't fail the test suite. Bounded by
// `maxRetries` so a permanently-held lock surfaces as a failure instead of
// hanging the runner.
const rm: typeof rmAsync = async (dir, opts) => {
  const maxRetries = (opts?.maxRetries ?? 0) + 4; // a few extra beyond node's own
  for (let attempt = 0; ; attempt++) {
    try {
      return await rmAsync(dir, opts);
    } catch (err) {
      const code = (err as NodeJS.ErrnoException)?.code;
      if (code === 'ENOENT') return;
      if ((code === 'EBUSY' || code === 'ENOTEMPTY') && attempt < maxRetries) {
        await new Promise((resolve) => setTimeout(resolve, 100));
        continue;
      }
      throw err;
    }
  }
};

const cleanup = (dir: string) => rm(dir, { recursive: true, force: true, maxRetries: 3 });

test('Engine.dispose(): subsequent method calls throw Disposed', async () => {
  const dir = mkdtempSync(join(tmpdir(), 'msb-dispose-'));
  try {
    const e = new Engine({ dataDir: dir });
    e.dispose();
    await assert.rejects(
      () => e.listIndexes(),
      (err: unknown) => {
        assert.ok(err instanceof MeilisearchBridgeError);
        assert.equal(err.code, 'Disposed');
        return true;
      },
    );
    await assert.rejects(
      () => e.getIndex('books', 'id'),
      (err: unknown) => {
        assert.ok(err instanceof MeilisearchBridgeError);
        assert.equal(err.code, 'Disposed');
        return true;
      },
    );
  } finally {
    await cleanup(dir);
  }
});

test('Index.dispose(): subsequent method calls throw Disposed', async () => {
  const dir = mkdtempSync(join(tmpdir(), 'msb-dispose-'));
  try {
    const e = new Engine({ dataDir: dir });
    const index = await e.getIndex('books', 'id');
    index.dispose();
    await assert.rejects(
      () => index.getDocuments(),
      (err: unknown) => {
        assert.ok(err instanceof MeilisearchBridgeError);
        assert.equal(err.code, 'Disposed');
        return true;
      },
    );
    await assert.rejects(
      () => index.search('whatever'),
      (err: unknown) => {
        assert.ok(err instanceof MeilisearchBridgeError);
        assert.equal(err.code, 'Disposed');
        return true;
      },
    );
    // Index.dispose() drops only this handle's Arc; the engine's cache still
    // holds one, so dispose the engine too before cleanup can delete the dir.
    e.dispose();
  } finally {
    await cleanup(dir);
  }
});

test('dispose() is idempotent and does not throw', () => {
  const dir = mkdtempSync(join(tmpdir(), 'msb-dispose-'));
  try {
    const e = new Engine({ dataDir: dir });
    e.dispose();
    e.dispose();
    e.dispose();
    assert.ok(true, 'repeated dispose() must not throw');
  } finally {
    rmAsync(dir, { recursive: true, force: true }).catch(() => {});
  }
});

test('Engine.dispose() does not affect outstanding Index handles', async () => {
  const dir = mkdtempSync(join(tmpdir(), 'msb-dispose-'));
  try {
    const e = new Engine({ dataDir: dir });
    const index = await e.getIndex('books', 'id');
    e.dispose();
    // Index handle is independent — querying it must still work.
    const docs = await index.getDocuments();
    assert.equal(docs.total, 0);
    index.dispose();
  } finally {
    await cleanup(dir);
  }
});

test('in-flight background indexing completes after Index.dispose()', async () => {
  const dir = mkdtempSync(join(tmpdir(), 'msb-dispose-'));
  try {
    const client = new Client({ dataDir: dir });
    const index = await client.createIndex('movies', { primaryKey: 'id' });
    const task = await index.addDocuments([{ id: '1', title: 'Inception' }]);
    // Dispose immediately — the background thread holds its own Arc and must
    // still finish the task. The engine survives to wait on the task.
    index.dispose();
    const waited = await client.waitForTask(task.taskUid, 10_000);
    assert.equal(waited.status, 'succeeded');
    assert.equal(waited.details?.indexedDocuments, 1);
    // Dispose the engine so its cached Arc<Mutex<milli::Index>> is dropped,
    // then let the background thread finish exiting before cleanup. Without
    // this, LMDB keeps the lock.mdb handle and the dir rm hangs on Windows.
    client.engine.dispose();
    await new Promise((resolve) => setTimeout(resolve, 50));
  } finally {
    await cleanup(dir);
  }
});

test('Symbol.dispose works with `using` syntax', async () => {
  const dir = mkdtempSync(join(tmpdir(), 'msb-dispose-'));
  let engine: Engine | null = null;
  try {
    {
      using e = new Engine({ dataDir: dir });
      engine = e;
      const idx = await e.getIndex('books', 'id');
      using _idx = idx;
      const docs = await idx.getDocuments();
      assert.equal(docs.total, 0);
    }
    // Scope exited — both handles disposed.
    await assert.rejects(
      () => engine!.listIndexes(),
      (err: unknown) => {
        assert.ok(err instanceof MeilisearchBridgeError);
        assert.equal(err.code, 'Disposed');
        return true;
      },
    );
  } finally {
    await cleanup(dir);
  }
});

test('data directory is deletable without EBUSY after disposing all handles', async () => {
  const dir = mkdtempSync(join(tmpdir(), 'msb-dispose-'));
  const e = new Engine({ dataDir: dir });
  const index = await e.getIndex('books', 'id');
  const task = await index.addDocuments([{ id: '1', title: 'A' }]);
  // Wait for the background indexer to finish AND exit its thread — the
  // detached `std::thread::spawn` keeps its own Arc<Mutex<milli::Index>>
  // alive until it returns, so we must let it drop before disposal can
  // actually close the LMDB env.
  await e.waitForTask(task.taskUid, 10_000);
  await new Promise((resolve) => setTimeout(resolve, 50));
  index.dispose();
  e.dispose();
  // Disposal released every Arc<Mutex<milli::Index>> the engine was holding,
  // and the background thread has exited — so the plain rm (no retry loop)
  // should succeed without EBUSY.
  await rmAsync(dir, { recursive: true, force: true });
});
