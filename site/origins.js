export const canonicalOrigin = "https://yotomediamanager.app";

export const redirectedHosts = new Set([
  "www.yotomediamanager.app",
  "yotomediamanager.damiengoehrig.ca",
  "yoto-media-manager.damien-goehrig.workers.dev",
]);

// Older Cloudflare build settings may still supply the retired SITE_URL.
// Keep explicit development origins, but never publish old production canonicals.
export function buildOrigin(value = canonicalOrigin) {
  const url = new URL(value);
  return redirectedHosts.has(url.hostname) ? canonicalOrigin : url.origin;
}
