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

test('Index: documentCount starts at 0 and addDocuments returns an enqueued task summary', async () => {
  const dir = mkdtempSync(join(tmpdir(), 'msb-'));
  try {
    const e = new Engine({ dataDir: dir });
    const idx = await e.getIndex('books', 'id');
    const { total } = await idx.getDocuments();
    assert.equal(total, 0);
    const task = await idx.addDocuments([
      { id: '1', title: 'A' },
      { id: '2', title: 'B' },
    ]);
    assert.equal(task.status, 'enqueued');
    assert.equal(task.type, 'documentAdditionOrUpdate');
    assert.equal(task.indexUid, 'books');
    assert.equal(typeof task.taskUid, 'number');
    assert.equal(typeof task.enqueuedAt, 'string');
    assert.equal('acceptedDocuments' in task, false);
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

    assert.equal(task.status, 'enqueued');
    assert.equal(task.type, 'documentAdditionOrUpdate');
    assert.equal(task.indexUid, 'movies');

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
    assert.equal(task.status, 'enqueued');
    assert.equal(task.type, 'settingsUpdate');
    assert.equal(task.indexUid, 'books');
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

test('Index: getDocuments supports minimal offset/limit/fields options', async () => {
  const dir = mkdtempSync(join(tmpdir(), 'msb-'));
  try {
    const client = new Client({ dataDir: dir });
    const index = await client.createIndex('movies', { primaryKey: 'id' });

    await client.waitForTask(
      (
        await index.addDocuments([
          { id: '1', title: 'Inception', genre: 'sci-fi', year: 2010 },
          { id: '2', title: 'Interstellar', genre: 'sci-fi', year: 2014 },
        ])
      ).taskUid,
    );

    const docs = await index.getDocuments({
      offset: 1,
      limit: 1,
      fields: ['title'],
    });

    assert.equal(docs.total, 2);
    assert.equal(docs.offset, 1);
    assert.equal(docs.limit, 1);
    assert.equal(docs.results.length, 1);
    assert.deepEqual(docs.results[0], { title: 'Interstellar' });
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test('Index: search supports minimal offset/limit/attributesToRetrieve options', async () => {
  const dir = mkdtempSync(join(tmpdir(), 'msb-'));
  try {
    const client = new Client({ dataDir: dir });
    const index = await client.createIndex('movies', { primaryKey: 'id' });

    await client.waitForTask(
      (
        await index.addDocuments([
          { id: '1', title: 'Alpha', genre: 'sci-fi', overview: 'space travel' },
          { id: '2', title: 'Beta', genre: 'sci-fi', overview: 'space station' },
        ])
      ).taskUid,
    );

    const results = await index.search('space', {
      offset: 1,
      limit: 1,
      attributesToRetrieve: ['title'],
    });

    assert.equal(results.estimatedTotalHits, 2);
    assert.equal(results.hits.length, 1);
    assert.equal(typeof results.hits[0]?.id, 'string');
    assert.equal(typeof results.hits[0]?._rankingScore, 'number');
    assert.equal(results.hits[0]?.genre, undefined);
    assert.equal(results.hits[0]?.overview, undefined);
    assert.ok(['Alpha', 'Beta'].includes(results.hits[0]?.title ?? ''));
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});
