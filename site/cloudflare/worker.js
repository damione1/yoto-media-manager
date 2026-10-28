import { securityHeaders } from "../security-headers.js";
import { canonicalOrigin, redirectedHosts } from "../origins.js";

export default {
  async fetch(request, env) {
    const url = new URL(request.url);
    if (redirectedHosts.has(url.hostname)
      || (url.origin === canonicalOrigin.replace("https:", "http:"))) {
      // Set the destination origin explicitly: a path starting // must never
      // be interpreted as another host. Keep the original path and query.
      const destination = new URL(canonicalOrigin);
      destination.pathname = url.pathname;
      destination.search = url.search;
      return Response.redirect(destination.href, 301);
    }

    // HTML is fetched from the asset cache, then gets a fresh policy per
    // response. An old browser validator must not turn it into a bodyless 304.
    let response = await env.ASSETS.fetch(request);
    if (response.status === 304) {
      const assetRequest = new Request(request);
      assetRequest.headers.delete("If-None-Match");
      assetRequest.headers.delete("If-Modified-Since");
      const unconditional = await env.ASSETS.fetch(assetRequest);
      // Static assets retain their original conditional response and caching.
      if (!unconditional.headers.get("Content-Type")?.startsWith("text/html")) return response;
      response = unconditional;
    }
    if (!response.headers.get("Content-Type")?.startsWith("text/html")) return response;

    const headers = new Headers(response.headers);
    for (const [name, value] of Object.entries(securityHeaders)) headers.set(name, value);
    const nonce = btoa(String.fromCharCode(...crypto.getRandomValues(new Uint8Array(18))));
    headers.set("Content-Security-Policy", securityHeaders["Content-Security-Policy"]
      .replace("script-src 'self'", `script-src 'self' 'nonce-${nonce}'`));
    // Cloudflare reads the nonce from this header and adds it to its injected
    // JavaScript Detections script. Do not reuse a nonce through browser caches.
    headers.set("Cache-Control", "no-store");
    headers.delete("ETag");
    headers.delete("Last-Modified");
    return new Response(response.body, { status: response.status, statusText: response.statusText, headers });
  },
};
