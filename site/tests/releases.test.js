import test from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {parseRelease, fetchReleaseRecord, downloadTargets} from '../releases.js';
const record = JSON.parse(readFileSync(new URL('./fixtures/release.json', import.meta.url)));

test('published installers have exact version, platform and architecture', () => {
  const release = parseRelease(record);
  assert.equal(release.version, record.tag.replace(/^v/, ''));
  assert.deepEqual(Object.values(release.downloads).map(d => [d.platform,d.architecture]),
    [['macos','arm64'],['macos','x86_64'],['windows','x86_64']]);
  for (const alter of [r => r.tag='v9.9.9', r => delete r.downloads.windows,
    r => r.downloads.extra=r.downloads.mac, r => r.downloads.mac+='?tracking=1',
    r => r.downloads.mac=r.downloads.mac.replace('github.com','example.com'), r => r.tag='v1.2.3-beta']) {
    const bad = structuredClone(record); alter(bad); assert.throws(() => parseRelease(bad));
  }
});

test('release resolution refuses incomplete or prerelease assets', async () => {
  const original = globalThis.fetch;
  const release = {tag_name:record.tag,draft:false,prerelease:false,
    assets:Object.entries(downloadTargets).map(([key,target]) => ({name:target.filename,size:10,browser_download_url:record.downloads[key]}))};
  try {
    globalThis.fetch = async () => ({ok:true,json:async () => release});
    assert.deepEqual(await fetchReleaseRecord(''),record);
    release.prerelease=true;
    await assert.rejects(fetchReleaseRecord(''), /stable/);
    release.prerelease=false; release.assets.pop();
    await assert.rejects(fetchReleaseRecord(''), /exactly one/);
  } finally { globalThis.fetch=original; }
});
