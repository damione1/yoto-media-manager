// SPDX-License-Identifier: GPL-3.0-or-later
export const downloadTargets = Object.freeze({
  mac: {filename:'YotoMediaManager-macOS-AppleSilicon.dmg',platform:'macos',architecture:'arm64'},
  macIntel: {filename:'YotoMediaManager-macOS-Intel.dmg',platform:'macos',architecture:'x86_64'},
  windows: {filename:'YotoMediaManager-Windows-setup.exe',platform:'windows',architecture:'x86_64'},
});

export function parseRelease(record) {
  if (record.schemaVersion !== 1 || !/^v?\d+\.\d+\.\d+$/.test(record.tag) ||
      Object.keys(record.downloads || {}).sort().join(',') !== Object.keys(downloadTargets).sort().join(',')) {
    throw new Error('Invalid published website release metadata');
  }
  const downloads = Object.fromEntries(Object.entries(downloadTargets).map(([key,target]) => {
    const url = new URL(record.downloads[key]);
    if (url.origin !== 'https://github.com' ||
        url.pathname !== `/damione1/yoto-media-manager/releases/download/${record.tag}/${target.filename}` || url.search || url.hash) {
      throw new Error(`Installer ${key} does not belong to the recorded release`);
    }
    return [key,{url:url.href,platform:target.platform,architecture:target.architecture,channel:'stable'}];
  }));
  return {tag:record.tag,version:record.tag.replace(/^v/,''),downloads};
}

export async function fetchReleaseRecord(token = process.env.GH_TOKEN || process.env.GITHUB_TOKEN) {
  const headers = {Accept:'application/vnd.github+json','User-Agent':'yotomediamanager-website-build'};
  if (token) headers.Authorization = `Bearer ${token}`;
  const response = await fetch('https://api.github.com/repos/damione1/yoto-media-manager/releases/latest',{
    headers,signal:AbortSignal.timeout(15000),
  });
  if (!response.ok) throw new Error(`Cannot resolve the published release (${response.status})`);
  const release = await response.json();
  if (release.draft || release.prerelease || !Array.isArray(release.assets)) throw new Error('Expected a published stable release');
  const downloads = Object.fromEntries(Object.entries(downloadTargets).map(([key,target]) => {
    const assets = release.assets.filter(asset => asset.name === target.filename && asset.size > 0);
    if (assets.length !== 1) throw new Error(`Expected exactly one published ${target.filename}`);
    return [key,assets[0].browser_download_url];
  }));
  const record = {schemaVersion:1,tag:release.tag_name,downloads};
  parseRelease(record);
  return record;
}
