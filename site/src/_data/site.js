/**
 * Constants shared by both languages.
 *
 * `url` is the public origin used for canonical links, hreflang, Open Graph
 * images and the sitemap. Cloudflare's build passes it in as SITE_URL (a
 * build variable of the Worker); without it the build falls back to the
 * site's public domain.
 */
import { buildOrigin } from "../../origins.js";
import release from "./release.js";

const repo = "https://github.com/damione1/yoto-media-manager";

export default async function () {
  const published = await release();
  return {
  name: "YotoMediaManager",
  url: buildOrigin(process.env.SITE_URL || undefined),
  repo,
  releases: `${repo}/releases`,
  latest: `${repo}/releases/latest`,
  // The resolved release keeps the file URL and its measured version together.
  download: Object.fromEntries(Object.entries(published.downloads).map(([key,asset]) => [key,asset.url])),
  issues: `${repo}/issues`,
  license: `${repo}/blob/master/LICENSE`,
  notices: `${repo}/blob/master/distribution/THIRD-PARTY-NOTICES.md`,
  author: "Damien Goehrig",
  authorUrl: "https://damiengoehrig.ca/",
  year: new Date().getFullYear(),
  };
}
