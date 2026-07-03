import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { Engine, MeilisearchBridgeError } from '../src/index.ts';

test('Engine: createIndex + listIndexes round-trip', async () => {
  const dir = mkdtempSync(join(tmpdir(), 'msb-'));
  try {
    const e = new Engine({ dataDir: dir });
    await e.createIndex('movies', { primaryKey: 'id' });
    const list = await e.listIndexes();
    assert.deepEqual(list, ['movies']);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test('Engine: creating an existing index throws IndexAlreadyExists', async () => {
  const dir = mkdtempSync(join(tmpdir(), 'msb-'));
  try {
    const e = new Engine({ dataDir: dir });
    await e.createIndex('dup', { primaryKey: 'id' });
    await assert.rejects(
      () => e.createIndex('dup', { primaryKey: 'id' }),
      (err: unknown) => {
        assert.ok(err instanceof MeilisearchBridgeError);
        assert.equal(err.code, 'IndexAlreadyExists');
        return true;
      },
    );
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test('Index: documentCount starts at 0 and addDocuments reports accepted count', async () => {
  const dir = mkdtempSync(join(tmpdir(), 'msb-'));
  try {
    const e = new Engine({ dataDir: dir });
    const idx = await e.getIndex('books', 'id');
    const { total } = await idx.getDocuments();
    assert.equal(total, 0);
    const r = await idx.addDocuments([
      { id: '1', title: 'A' },
      { id: '2', title: 'B' },
    ]);
    assert.equal(r.acceptedDocuments, 2);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});