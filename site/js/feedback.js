// The feedback form: what's written goes to the site's own /api/feedback and nowhere else.
import { LANG } from './site-data.js';

const TEXT = {
  en: {
    kicker: 'FEEDBACK', title: 'Something wrong, or an idea?', text: 'What happened, or what would you like Magpie to do?',
    contact: 'How to reach you, if you want an answer (optional)', send: 'Send', sending: 'Sending…',
    note: 'Only what you write here is sent, with the version number.', thanks: 'Sent. Thank you.',
    failed: "That didn't go through. Try again in a while.", many: 'That is enough for today. Thank you.',
  },
  vi: {
    kicker: 'GÓP Ý', title: 'Gặp lỗi, hay có ý tưởng?', text: 'Chuyện gì đã xảy ra, hoặc bạn muốn Magpie làm được gì?',
    contact: 'Cách liên lạc với bạn, nếu bạn muốn được trả lời (không bắt buộc)', send: 'Gửi', sending: 'Đang gửi…',
    note: 'Chỉ những gì bạn viết ở đây được gửi đi, kèm số phiên bản.', thanks: 'Đã gửi. Cảm ơn bạn.',
    failed: 'Chưa gửi được. Bạn thử lại sau nhé.', many: 'Hôm nay như vậy là đủ rồi. Cảm ơn bạn.',
  },
}[LANG === 'vi' ? 'vi' : 'en'];

const form = document.getElementById('fb-form');
if (form) {
  const get = (id) => document.getElementById(id);
  get('fb-kicker').textContent = TEXT.kicker;
  get('fb-title').textContent = TEXT.title;
  get('fb-text').placeholder = TEXT.text;
  get('fb-contact').placeholder = TEXT.contact;
  get('fb-send').textContent = TEXT.send;
  get('fb-note').textContent = TEXT.note;
  form.addEventListener('submit', async (event) => {
    event.preventDefault();
    const send = get('fb-send');
    send.disabled = true;
    send.textContent = TEXT.sending;
    let said = TEXT.failed;
    try {
      const reply = await fetch('/api/feedback', {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify({
          text: get('fb-text').value, contact: get('fb-contact').value, language: LANG,
          version: new URLSearchParams(location.search).get('v') || '',
        }),
      });
      if (reply.ok) {
        said = TEXT.thanks;
        form.reset();
      } else if (reply.status === 429) said = TEXT.many;
    } catch {
      /* said stays "failed" */
    }
    get('fb-note').textContent = said;
    send.disabled = false;
    send.textContent = TEXT.send;
  });
}
