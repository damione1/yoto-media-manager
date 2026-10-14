/**
 * Constants shared by both languages.
 *
 * `url` is the public origin used for canonical links, hreflang, Open Graph
 * images and the sitemap. The deploy workflow passes it in as SITE_URL (from
 * the `SITE_URL` repository variable); without it the build falls back to the
 * Cloudflare Pages default domain of the `yotomediamanager` project.
 */
const repo = "https://github.com/damione1/yoto-media-manager";

export default {
  name: "YotoMediaManager",
  url: (process.env.SITE_URL || "https://yotomediamanager.pages.dev").replace(/\/+$/, ""),
  repo,
  releases: `${repo}/releases`,
  latest: `${repo}/releases/latest`,
  issues: `${repo}/issues`,
  notices: `${repo}/blob/master/distribution/THIRD-PARTY-NOTICES.md`,
  author: "Damien Goehrig",
  year: new Date().getFullYear(),
};
