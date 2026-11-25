// The website is files, except for one thing: feedback, which is written to a store that only the site's owner
// can read. A message is at most 4000 characters, and one address can leave five a day.

const DAY = 86400;

async function hash(text) {
  const bytes = await crypto.subtle.digest('SHA-256', new TextEncoder().encode(text));
  return [...new Uint8Array(bytes)].slice(0, 12).map((byte) => byte.toString(16).padStart(2, '0')).join('');
}

// The day's salt: random, made by the first message of the day, kept no longer than that day's counts.
async function salt(env, today) {
  const key = `salt:${today}`;
  let value = await env.FEEDBACK.get(key);
  if (!value) {
    value = [...crypto.getRandomValues(new Uint8Array(16))].map((byte) => byte.toString(16).padStart(2, '0')).join('');
    await env.FEEDBACK.put(key, value, { expirationTtl: 2 * DAY });
  }
  return value;
}

// The body, but never more than `limit` bytes of it: a sender that leaves out the length (or lies about it) is cut
// off rather than read whole.
async function capped(request, limit) {
  if (!request.body) return '';
  const reader = request.body.getReader();
  const chunks = [];
  let size = 0;
  for (;;) {
    const { done, value } = await reader.read();
    if (done) break;
    size += value.byteLength;
    if (size > limit) {
      await reader.cancel();
      return null;
    }
    chunks.push(value);
  }
  const all = new Uint8Array(size);
  let at = 0;
  for (const chunk of chunks) {
    all.set(chunk, at);
    at += chunk.byteLength;
  }
  return new TextDecoder().decode(all);
}

async function feedback(request, env) {
  if (request.method !== 'POST') return new Response('POST only', { status: 405 });
  // Only the site's own page posts here.
  const origin = request.headers.get('origin');
  if (origin && origin !== new URL(request.url).origin) return new Response('no', { status: 403 });
  if (Number(request.headers.get('content-length') || 0) > 6000) return new Response('too long', { status: 413 });
  const raw = await capped(request, 6000);
  if (raw === null) return new Response('too long', { status: 413 });
  let body;
  try {
    body = JSON.parse(raw);
  } catch {
    return new Response('not JSON', { status: 400 });
  }
  if (!body || typeof body !== 'object') return new Response('not JSON', { status: 400 });
  const text = String(body.text || '').trim().slice(0, 4000);
  if (text.length < 3) return new Response('empty', { status: 400 });
  const today = new Date().toISOString().slice(0, 10);
  // The address itself isn't kept: only a hash of it with the day's random salt, to count. There are few enough
  // addresses to try them all against a plain hash; the salt is gone with the counts, two days later.
  const who = await hash(`${request.headers.get('cf-connecting-ip') || ''}|${today}|${await salt(env, today)}`);
  const count = Number((await env.FEEDBACK.get(`rate:${who}`)) || 0);
  if (count >= 5) return new Response('enough for today', { status: 429 });
  await env.FEEDBACK.put(`rate:${who}`, String(count + 1), { expirationTtl: 2 * DAY });
  const key = `fb:${new Date().toISOString()}:${crypto.randomUUID().slice(0, 8)}`;
  await env.FEEDBACK.put(key, JSON.stringify({
    text,
    contact: String(body.contact || '').slice(0, 200),
    version: String(body.version || '').slice(0, 20),
    language: String(body.language || '').slice(0, 5),
  }));
  return new Response('ok');
}

// The app's model and tool files, from the site's own store: the first place the app looks on the web, before the
// places their makers publish them. The app checks every file's SHA-256 itself, so this copy only has to be there.
async function model(request, env) {
  if (request.method !== 'GET' && request.method !== 'HEAD') return new Response('GET only', { status: 405 });
  const name = new URL(request.url).pathname.slice('/models/'.length);
  if (!/^[A-Za-z0-9][A-Za-z0-9._-]{0,127}$/.test(name)) return new Response('not here', { status: 404 });
  const found = await env.MODELS.head(name);
  if (!found) return new Response('not here', { status: 404 });
  const headers = new Headers({
    'content-type': 'application/octet-stream',
    'accept-ranges': 'bytes',
    etag: found.httpEtag,
    'cache-control': 'public, max-age=31536000, immutable',
  });
  // Only `bytes=from-` and `bytes=from-to`, which is all a download that goes on needs; anything else gets it whole.
  const asked = /^bytes=(\d+)-(\d*)$/.exec(request.headers.get('range') || '');
  let offset = 0;
  let length = found.size;
  if (asked) {
    offset = Number(asked[1]);
    const last = asked[2] === '' ? found.size - 1 : Math.min(Number(asked[2]), found.size - 1);
    if (offset >= found.size || last < offset) {
      return new Response('no such range', { status: 416, headers: { 'content-range': `bytes */${found.size}` } });
    }
    length = last - offset + 1;
    headers.set('content-range', `bytes ${offset}-${last}/${found.size}`);
  }
  headers.set('content-length', String(length));
  const status = asked ? 206 : 200;
  if (request.method === 'HEAD') return new Response(null, { status, headers });
  const object = await env.MODELS.get(name, asked ? { range: { offset, length } } : {});
  if (!object) return new Response('not here', { status: 404 });
  return new Response(object.body, { status, headers });
}

export default {
  async fetch(request, env) {
    const path = new URL(request.url).pathname;
    if (path === '/api/feedback') return feedback(request, env);
    if (path.startsWith('/models/')) return model(request, env);
    return env.ASSETS.fetch(request);
  },
};
