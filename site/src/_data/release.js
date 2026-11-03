// SPDX-License-Identifier: GPL-3.0-or-later
import {fetchReleaseRecord, parseRelease} from '../../releases.js';
let published;
// Resolve once per build, so every URL and measured version use the same release.
export default function release() {
  return published ??= fetchReleaseRecord().then(parseRelease);
}
