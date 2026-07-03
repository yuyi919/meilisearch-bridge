import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { Client, Engine, MeilisearchBridgeError } from '../src/index.ts';

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

test('Index: addDocuments writes searchable data and waitForTask returns a completed task', async () => {
  const dir = mkdtempSync(join(tmpdir(), 'msb-'));
  try {
    const client = new Client({ dataDir: dir });
    const index = await client.createIndex('movies', { primaryKey: 'id' });

    const task = await index.addDocuments([
      { id: '1', title: 'Inception', genre: 'sci-fi' },
      { id: '2', title: 'Interstellar', genre: 'sci-fi' },
    ]);

    const waited = await client.waitForTask(task.taskUid);
    assert.equal(waited.status, 'succeeded');
    assert.equal(waited.type, 'documentAdditionOrUpdate');
    assert.equal(waited.indexUid, 'movies');
    assert.equal(waited.details?.receivedDocuments, 2);
    assert.equal(waited.details?.indexedDocuments, 2);

    const fetched = await client.getTask(task.taskUid);
    assert.equal(fetched.uid, task.taskUid);
    assert.equal(fetched.status, 'succeeded');

    const search = await index.search('interstellar');
    assert.equal(search.hits.length, 1);
    assert.equal(search.hits[0]?.id, '2');
    assert.equal(search.hits[0]?.title, 'Interstellar');
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test('Index: updateSettings(searchableAttributes) changes searchable fields for subsequent searches', async () => {
  const dir = mkdtempSync(join(tmpdir(), 'msb-'));
  try {
    const client = new Client({ dataDir: dir });
    const index = await client.createIndex('books', { primaryKey: 'id' });

    await client.waitForTask(
      (
        await index.addDocuments([
          { id: '1', title: 'Dune', overview: 'Spice on Arrakis' },
          { id: '2', title: 'Foundation', overview: 'Psychohistory and empire' },
        ])
      ).taskUid,
    );

    const before = await index.search('arrakis');
    assert.equal(before.hits.length, 1);
    assert.equal(before.hits[0]?.id, '1');

    const task = await index.updateSettings({
      searchableAttributes: ['title'],
    });
    const waited = await client.waitForTask(task.taskUid);
    assert.equal(waited.status, 'succeeded');
    assert.equal(waited.type, 'settingsUpdate');

    const afterOverview = await index.search('arrakis');
    assert.equal(afterOverview.hits.length, 0);

    const afterTitle = await index.search('dune');
    assert.equal(afterTitle.hits.length, 1);
    assert.equal(afterTitle.hits[0]?.id, '1');
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});
