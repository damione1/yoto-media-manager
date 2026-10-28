/**
 * Constants shared by both languages.
 *
 * `url` is the public origin used for canonical links, hreflang, Open Graph
 * images and the sitemap. Cloudflare's build passes it in as SITE_URL (a
 * build variable of the Worker); without it the build falls back to the
 * site's public domain.
 */
import { buildOrigin } from "../../origins.js";

const repo = "https://github.com/damione1/yoto-media-manager";

export default {
  name: "YotoMediaManager",
  url: buildOrigin(process.env.SITE_URL || undefined),
  repo,
  releases: `${repo}/releases`,
  latest: `${repo}/releases/latest`,
  // Every release also publishes its installers under these fixed names.
  download: {
    mac: `${repo}/releases/latest/download/YotoMediaManager-macOS-AppleSilicon.dmg`,
    macIntel: `${repo}/releases/latest/download/YotoMediaManager-macOS-Intel.dmg`,
    windows: `${repo}/releases/latest/download/YotoMediaManager-Windows-setup.exe`,
  },
  issues: `${repo}/issues`,
  license: `${repo}/blob/master/LICENSE`,
  notices: `${repo}/blob/master/distribution/THIRD-PARTY-NOTICES.md`,
  author: "Damien Goehrig",
  year: new Date().getFullYear(),
};
