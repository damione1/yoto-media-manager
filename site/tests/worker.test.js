import test from "node:test";
import assert from "node:assert/strict";
import worker from "../cloudflare/worker.js";
import { canonicalOrigin, redirectedHosts, buildOrigin } from "../origins.js";

const htmlHeaders = { "Content-Type": "text/html; charset=utf-8", ETag: '"old"', "Last-Modified": "Tue, 27 Oct 2026 00:00:00 GMT" };

test("HTML responses have fresh CSP nonces without relaxing inline-script restrictions", async () => {
  const env = { ASSETS: { fetch: async (request) => {
    if (request.headers.has("If-None-Match") || request.headers.has("If-Modified-Since")) {
      return new Response(null, { status: 304, headers: htmlHeaders });
    }
    assert.equal(request.headers.has("If-None-Match"), false);
    assert.equal(request.headers.has("If-Modified-Since"), false);
    return new Response("<h1>Yoto</h1>", { headers: htmlHeaders });
  } } };
  const request = new Request("https://example.com/fr/", { headers: { "If-None-Match": '"old"', "If-Modified-Since": htmlHeaders["Last-Modified"] } });
  const first = await worker.fetch(request, env);
  const second = await worker.fetch(request, env);
  const policy = first.headers.get("Content-Security-Policy");
  const nonce = policy.match(/'nonce-([^']+)'/)[1];
  assert.equal(Buffer.from(nonce, "base64").length, 18);
  assert.notEqual(policy, second.headers.get("Content-Security-Policy"));
  assert.ok(policy.includes("script-src 'self' 'nonce-"));
  assert.ok(!policy.includes("unsafe-inline"));
  assert.ok(policy.includes("object-src 'none'"));
  assert.equal(first.headers.get("Cache-Control"), "no-store");
  assert.equal(first.headers.has("ETag"), false);
  assert.equal(first.headers.has("Last-Modified"), false);
  assert.equal(await first.text(), "<h1>Yoto</h1>");
});

test("retired domains redirect pages and assets with their path and query intact", async () => {
  const env = { ASSETS: { fetch: async () => { throw new Error("Redirects must not fetch assets"); } } };
  for (const host of redirectedHosts) {
    for (const path of ["/", "/fr/docs/imports/?source=old&name=a%2Fb", "/sitemap.xml", "/css/site.css?v=123", "//example.org/path", "/fr/docs/inconnu/"]) {
      for (const method of ["GET", "HEAD"]) {
        const response = await worker.fetch(new Request(`https://${host}${path}`, { method }), env);
        assert.equal(response.status, 301);
        assert.equal(response.headers.get("Location"), `${canonicalOrigin}${path}`);
        assert.equal(response.body, null);
      }
    }
  }
});

test("primary domain and local preview serve assets without a domain redirect", async () => {
  for (const origin of [canonicalOrigin, "http://localhost:8787"]) {
    const response = await worker.fetch(new Request(`${origin}/fr/`), {
      ASSETS: { fetch: async () => new Response("<h1>Yoto</h1>", { headers: htmlHeaders }) },
    });
    assert.equal(response.status, 200);
    assert.equal(response.headers.has("Location"), false);
  }
});

test("HTTP on the primary domain redirects directly to HTTPS", async () => {
  const path = "/fr/docs/imports/?source=http&name=a%2Fb";
  const response = await worker.fetch(new Request(`http://yotomediamanager.app${path}`), {
    ASSETS: { fetch: async () => { throw new Error("HTTP must redirect before fetching assets"); } },
  });
  assert.equal(response.status, 301);
  assert.equal(response.headers.get("Location"), `${canonicalOrigin}${path}`);
});

test("previous Cloudflare build variables cannot restore retired canonicals", () => {
  assert.equal(buildOrigin(), canonicalOrigin);
  for (const host of redirectedHosts) assert.equal(buildOrigin(`https://${host}/`), canonicalOrigin);
  assert.equal(buildOrigin("http://localhost:8787/"), "http://localhost:8787");
});

test("conditional static asset requests retain their 304 response", async () => {
  const original = new Response(null, { status: 304, headers: { ETag: '"asset"' } });
  const response = await worker.fetch(new Request(`${canonicalOrigin}/css/site.css`, { headers: { "If-None-Match": '"asset"' } }), {
    ASSETS: { fetch: async (request) => request.headers.has("If-None-Match") ? original
      : new Response("body {}", { headers: { "Content-Type": "text/css", ETag: '"asset"' } }) },
  });
  assert.equal(response, original);
});

test("custom HTML errors retain their 404 status", async () => {
  const response = await worker.fetch(new Request("https://example.com/missing"), {
    ASSETS: { fetch: async () => new Response("<h1>Missing</h1>", { status: 404, headers: htmlHeaders }) },
  });
  assert.equal(response.status, 404);
  assert.equal(await response.text(), "<h1>Missing</h1>");
  assert.match(response.headers.get("Content-Security-Policy"), /nonce-/);
});

test("static file caching and permanent redirects are preserved", async () => {
  for (const original of [
    new Response("body {}", { headers: { "Content-Type": "text/css", "Cache-Control": "public, max-age=31536000, immutable", ETag: '"asset"' } }),
    new Response(null, { status: 301, headers: { Location: "/fr/" } }),
  ]) {
    const response = await worker.fetch(new Request("https://example.com/file"), { ASSETS: { fetch: async () => original } });
    assert.equal(response, original);
  }
});

test("HEAD responses remain bodyless", async () => {
  const response = await worker.fetch(new Request("https://example.com/", { method: "HEAD" }), {
    ASSETS: { fetch: async () => new Response(null, { headers: htmlHeaders }) },
  });
  assert.equal(response.body, null);
});
