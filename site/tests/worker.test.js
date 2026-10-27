import test from "node:test";
import assert from "node:assert/strict";
import worker from "../cloudflare/worker.js";

const htmlHeaders = { "Content-Type": "text/html; charset=utf-8", ETag: '"old"', "Last-Modified": "Tue, 27 Oct 2026 00:00:00 GMT" };

test("HTML responses have fresh CSP nonces without relaxing inline-script restrictions", async () => {
  const env = { ASSETS: { fetch: async (request) => {
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
