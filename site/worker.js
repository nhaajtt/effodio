// The website is files, except for one thing: feedback, which is written to a store that only the site's owner
// can read. A message is at most 4000 characters, and one address can leave five a day.

const DAY = 86400;

async function hash(text) {
  const bytes = await crypto.subtle.digest('SHA-256', new TextEncoder().encode(text));
  return [...new Uint8Array(bytes)].slice(0, 12).map((byte) => byte.toString(16).padStart(2, '0')).join('');
}

async function feedback(request, env) {
  if (request.method !== 'POST') return new Response('POST only', { status: 405 });
  // Only the site's own page posts here.
  const origin = request.headers.get('origin');
  if (origin && origin !== new URL(request.url).origin) return new Response('no', { status: 403 });
  if (Number(request.headers.get('content-length') || 0) > 6000) return new Response('too long', { status: 413 });
  let body;
  try {
    body = JSON.parse((await request.text()).slice(0, 6000));
  } catch {
    return new Response('not JSON', { status: 400 });
  }
  const text = String(body.text || '').trim().slice(0, 4000);
  if (text.length < 3) return new Response('empty', { status: 400 });
  const today = new Date().toISOString().slice(0, 10);
  // The address itself isn't kept: only a hash of it with the day, to count.
  const who = await hash(`${request.headers.get('cf-connecting-ip') || ''}|${today}`);
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

export default {
  async fetch(request, env) {
    if (new URL(request.url).pathname === '/api/feedback') return feedback(request, env);
    return env.ASSETS.fetch(request);
  },
};
