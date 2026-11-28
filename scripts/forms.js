// Suppa Browser - addresses, cards, PayPal and passwords in web forms.
// Runs in every page and frame (Settings -> Addresses and payments -> Fill forms).
//  * A small key button appears in address, card, PayPal and password fields.
//    Clicking it opens Suppa's own menu (this page never sees your saved data);
//    only the entry you pick is filled in, by window.__suppaFill.
//  * Fields for a new password get "Use a strong password" (saved by Suppa).
//  * When a form with a new address or card is sent, Suppa offers to save it
//    (security codes / CVC are never read or saved).
// Edit, then press "Update" in the browser.
(function () {
  if (window.__suppaForms) return;
  window.__suppaForms = true;
  if (!/^https?:$/.test(location.protocol) || /(^|\.)suppa\.[a-z]+$/.test(location.hostname)) return;
  const send = (kind, obj) => { try { window.chrome.webview.postMessage(kind + ':' + JSON.stringify(obj)); } catch (e) {} };

  // ---- what is this field? ----------------------------------------------------
  const AC = {
    'name': 'name', 'given-name': 'first', 'family-name': 'last', 'organization': 'company',
    'street-address': 'street1', 'address-line1': 'street1', 'address-line2': 'street2',
    'address-level2': 'city', 'address-level1': 'region', 'postal-code': 'zip',
    'country': 'country', 'country-name': 'country', 'tel': 'phone', 'tel-national': 'phone', 'email': 'email',
    'cc-name': 'cc-name', 'cc-number': 'cc-number', 'cc-exp': 'cc-exp', 'cc-exp-month': 'cc-month',
    'cc-exp-year': 'cc-year', 'cc-csc': 'cvc', 'username': 'username', 'current-password': 'password',
    'new-password': 'new-password', 'one-time-code': 'otp',
  };
  const RX = [
    ['cvc', /cvc|cvv|csc|security.?code|card.?code|kartenpr/],
    ['cc-number', /card.?num|cc.?num|cardnumber|credit.?card|card.?no|номер.?карты|kartennummer|numero.?de.?tarjeta/],
    ['cc-name', /(card|cc).?(holder|name|owner)|name.?on.?card|nameoncard|karteninhaber/],
    ['cc-exp', /mm\s*\/?\s*(yy|jj|aa)|exp(iry|iration)?.?date|valid.?thru/],
    ['cc-month', /(exp|cc).*(month|mm\b)/],
    ['cc-year', /(exp|cc).*(year|yy)/],
    ['cc-exp', /expir|exp.?date|mm.?\/?.?yy|valid.?thru|ablauf|срок/],
    ['otp', /one.?time|otp|2fa|verification.?code|sms.?code/],
    ['email', /e.?mail|courriel|почта/],
    ['phone', /phone|mobile|\btel\b|telephone|handy|телефон/],
    ['zip', /zip|postal|post.?code|plz|cap\b|индекс/],
    ['city', /city|town|locality|\bort\b|stadt|ville|город/],
    ['region', /state|province|region|county|bundesland|область/],
    ['country', /country|\bland\b|pays|страна/],
    ['street2', /address.?(line)?.?2|addr.?2|apt|apartment|suite|\bunit\b|flat|building|квартира/],
    ['street1', /address|street|addr|line.?1|stra(ss|ß)e|adresse|адрес|улица/],
    ['company', /company|organi[sz]ation|business|firm|firma/],
    ['first', /first.?name|given|fname|vorname|prénom|имя/],
    ['last', /last.?name|surname|family|lname|nachname|фамилия/],
    ['name', /full.?name|your.?name|^name$|\bname\b|^fio|ф\.?и\.?о/],
    ['username', /user.?name|login|account|benutzer|логин/],
  ];
  const TYPES = new Set(['text', 'email', 'tel', 'password', 'number', 'search', '']);
  function hint(el) {
    let h = [el.name, el.id, el.placeholder, el.getAttribute('aria-label'), el.getAttribute('data-testid'), el.className].join(' ');
    if (el.labels && el.labels[0]) h += ' ' + el.labels[0].textContent;
    const lb = el.getAttribute('aria-labelledby');
    if (lb) lb.split(/\s+/).forEach((id) => { const x = document.getElementById(id); if (x) h += ' ' + x.textContent; });
    return h.toLowerCase().replace(/\s+/g, ' ');
  }
  function fieldOf(el) {
    if (el.tagName === 'SELECT') {
      const ac = (el.autocomplete || '').toLowerCase().split(/\s+/).pop();
      if (AC[ac]) return AC[ac];
      const h = hint(el);
      for (const [k, rx] of RX) if (['country', 'region', 'cc-month', 'cc-year'].includes(k) && rx.test(h)) return k;
      return '';
    }
    if (el.tagName !== 'INPUT' || !TYPES.has(el.type) || el.readOnly || el.disabled) return '';
    const ac = (el.getAttribute('autocomplete') || '').toLowerCase().split(/\s+/).filter((x) => x !== 'shipping' && x !== 'billing' && !x.startsWith('section-')).pop();
    if (ac && AC[ac]) return AC[ac];
    if (el.type === 'password') return isNewPassword(el) ? 'new-password' : 'password';
    if (el.type === 'email') return 'email';
    if (el.type === 'tel') return 'phone';
    const h = hint(el);
    for (const [k, rx] of RX) if (rx.test(h)) return k;
    return '';
  }
  function scope(el) { return el.form || el.closest('form') || document; }
  function passwordsIn(sc) { return [...sc.querySelectorAll('input[type=password]')].filter(visible); }
  function isNewPassword(el) {
    const ac = (el.getAttribute('autocomplete') || '').toLowerCase();
    if (ac.includes('new-password')) return true;
    if (ac.includes('current-password')) return false;
    if (/new|confirm|repeat|retype|again|create|register|sign.?up|choose|neu|wiederhol/.test(hint(el))) return true;
    if (passwordsIn(scope(el)).length >= 2) return true;
    return /regist|sign.?up|join|create.?account|new.?account/.test(location.pathname + ' ' + (scope(el).action || ''));
  }
  function visible(el) {
    const r = el.getBoundingClientRect();
    return r.width > 0 && r.height > 0 && getComputedStyle(el).visibility !== 'hidden';
  }
  const KIND = (f) => f.startsWith('cc-') ? 'card'
    : f === 'new-password' ? 'newpw'
    : f === 'password' || f === 'username' ? 'login'
    : f === 'cvc' || f === 'otp' || !f ? ''
    : 'address';
  function kindOf(el) {
    const f = fieldOf(el);
    let k = KIND(f);
    const sc = scope(el);
    const pws = passwordsIn(sc);
    if (/(^|\.)paypal\.com$/.test(location.hostname) && (f === 'email' || f === 'username')) return 'paypal';
    // A user name / e-mail field of a sign-in form is part of the login.
    if ((f === 'email' || f === 'username' || (f === '' && el.form)) && pws.length) k = pws.some(isNewPassword) ? 'newpw' : 'login';
    // A lone search / newsletter box is not an address form.
    if (k === 'address' && sc === document && !['email', 'phone'].includes(f)) k = '';
    if (k === 'newpw' && el.type !== 'password') k = 'login';
    return k;
  }
  function hasPaypal() {
    return !!document.querySelector('[data-funding-source="paypal"], .paypal-button, .paypal-buttons, iframe[src*="paypal.com"], [id*="paypal" i] button, button[aria-label*="PayPal" i], img[alt*="PayPal" i]');
  }

  // ---- the key button -----------------------------------------------------------
  let host = null, btn = null, current = null, hideT = 0;
  function makeButton() {
    host = document.createElement('suppa-fill');
    host.style.cssText = 'all:initial;position:fixed;z-index:2147483647;display:none;width:22px;height:22px';
    const root = host.attachShadow({ mode: 'closed' });
    btn = document.createElement('button');
    btn.title = 'Suppa: fill in';
    btn.style.cssText = 'all:initial;cursor:pointer;width:22px;height:22px;border-radius:6px;display:flex;align-items:center;justify-content:center;background:rgba(127,127,127,.18)';
    btn.innerHTML = '<svg viewBox="0 0 16 16" width="14" height="14" fill="none" stroke="#7a7a7a" stroke-width="1.6" stroke-linecap="round"><circle cx="5.5" cy="8" r="3"/><path d="M8.5 8h6M12.5 8v2.5M14.5 8v2"/></svg>';
    btn.addEventListener('mousedown', (e) => { e.preventDefault(); e.stopPropagation(); });
    btn.addEventListener('click', (e) => {
      e.preventDefault();
      e.stopPropagation();
      if (!current) return;
      const user = userField(scope(current));
      send('suppa-fill', { kind: current._suppaKind, user: user ? user.value : '', paypal: current._suppaKind === 'card' && hasPaypal() });
    });
    root.appendChild(btn);
    (document.documentElement || document.body).appendChild(host);
  }
  function place() {
    if (!current || !host) return;
    const r = current.getBoundingClientRect();
    if (!r.width || r.bottom < 0 || r.top > innerHeight) { host.style.display = 'none'; return; }
    host.style.left = Math.round(r.right - 26) + 'px';
    host.style.top = Math.round(r.top + (r.height - 22) / 2) + 'px';
    host.style.display = 'block';
  }
  document.addEventListener('focusin', (e) => {
    const el = e.target;
    if (!(el instanceof HTMLInputElement)) return;
    const k = kindOf(el);
    if (!k || el.getBoundingClientRect().width < 70) return;
    el._suppaKind = k;
    current = el;
    clearTimeout(hideT);
    if (!host || !host.isConnected) makeButton();
    place();
  }, true);
  document.addEventListener('focusout', () => {
    clearTimeout(hideT);
    hideT = setTimeout(() => { if (host) host.style.display = 'none'; current = null; }, 250);
  }, true);
  addEventListener('scroll', place, true);
  addEventListener('resize', place);

  // ---- filling ------------------------------------------------------------------
  const inputSetter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value').set;
  function put(el, v) {
    if (v == null || v === '' || !el || el.disabled || el.readOnly) return;
    if (el.tagName === 'SELECT') {
      const low = String(v).toLowerCase();
      const o = [...el.options].find((o) => o.value.toLowerCase() === low || o.text.trim().toLowerCase() === low)
        || [...el.options].find((o) => low.length > 2 && (o.text.toLowerCase().includes(low) || low.includes(o.text.trim().toLowerCase()) && o.text.trim().length > 2));
      if (!o) return;
      el.value = o.value;
    } else {
      el.focus();
      inputSetter.call(el, String(v));
      el.dispatchEvent(new InputEvent('input', { bubbles: true, inputType: 'insertText', data: String(v) }));
    }
    el.dispatchEvent(new Event('change', { bubbles: true }));
    el.style.transition = 'background-color .6s';
    el.style.backgroundColor = 'rgba(120,170,255,.22)';
    setTimeout(() => { el.style.backgroundColor = ''; }, 900);
  }
  function fieldsIn(sc) {
    return [...sc.querySelectorAll('input, select')].filter((el) => visible(el) || el.tagName === 'SELECT').map((el) => [el, fieldOf(el)]);
  }
  function userField(sc) {
    const pw = passwordsIn(sc)[0];
    const cands = [...sc.querySelectorAll('input')].filter((el) => visible(el) && ['text', 'email', 'tel', ''].includes(el.type) && el !== pw);
    const named = cands.find((el) => ['username', 'email'].includes(fieldOf(el)));
    if (named) return named;
    // The text field right before the password.
    if (pw) {
      const before = cands.filter((el) => el.compareDocumentPosition(pw) & Node.DOCUMENT_POSITION_FOLLOWING);
      return before[before.length - 1] || null;
    }
    return null;
  }
  function yearFor(el, y) {
    const four = String(y).length === 4 ? String(y) : '20' + String(y).padStart(2, '0');
    if (el.tagName === 'SELECT') return [...el.options].some((o) => o.value === four || o.text.trim() === four) ? four : four.slice(2);
    return el.maxLength === 2 || /yy(?!yy)/i.test(el.placeholder || '') ? four.slice(2) : four;
  }
  window.__suppaFill = function (kind, d) {
    const sc = current ? scope(current) : document;
    if (kind === 'paypal_button') {
      const b = document.querySelector('[data-funding-source="paypal"], .paypal-button, .paypal-buttons, button[aria-label*="PayPal" i], [id*="paypal" i] button, img[alt*="PayPal" i]');
      if (b) { b.scrollIntoView({ block: 'center' }); b.click(); }
      return;
    }
    if (kind === 'login' || kind === 'newpw') {
      const pws = passwordsIn(sc);
      if (kind === 'newpw') {
        pws.filter((p) => fieldOf(p) === 'new-password' || pws.length <= 2).forEach((p) => put(p, d.password));
      } else {
        const u = userField(sc);
        if (u && d.user) put(u, d.user);
        if (pws[0]) put(pws[0], d.password);
      }
      return;
    }
    if (kind === 'paypal') {
      if (current) put(current, d.email);
      return;
    }
    const [month, year] = String(d.exp || '').split(/\s*\/\s*/);
    const parts = String(d.name || '').trim().split(/\s+/);
    const first = parts.length > 1 ? parts.slice(0, -1).join(' ') : parts[0] || '';
    const last = parts.length > 1 ? parts[parts.length - 1] : '';
    for (const [el, f] of fieldsIn(sc)) {
      if (kind === 'card') {
        if (f === 'cc-number') put(el, d.number);
        else if (f === 'cc-name') put(el, d.name);
        else if (f === 'cc-exp' && month && year) put(el, month.padStart(2, '0') + (el.maxLength === 4 ? '' : '/') + String(year).slice(-2));
        else if (f === 'cc-month' && month) put(el, el.tagName === 'SELECT' && ![...el.options].some((o) => o.value === month.padStart(2, '0')) ? String(+month) : month.padStart(2, '0'));
        else if (f === 'cc-year' && year) put(el, yearFor(el, year));
      } else {
        const map = { name: d.name, first, last, company: d.company, street1: d.street1, street2: d.street2, city: d.city, region: d.region, zip: d.zip, country: d.country, phone: d.phone, email: d.email };
        if (map[f]) put(el, map[f]);
      }
    }
  };

  // ---- sent forms: offer to save ---------------------------------------------
  function luhn(n) {
    let sum = 0;
    for (let i = 0; i < n.length; i++) {
      let x = +n[n.length - 1 - i];
      if (i % 2) { x *= 2; if (x > 9) x -= 9; }
      sum += x;
    }
    return n.length >= 12 && sum % 10 === 0;
  }
  let lastSent = 0;
  function collect(sc) {
    if (Date.now() - lastSent < 1500) return;
    const a = {}, c = {};
    for (const [el, f] of fieldsIn(sc)) {
      const v = (el.tagName === 'SELECT' ? (el.selectedOptions[0] || {}).text || '' : el.value || '').trim();
      if (!v || el.type === 'password') continue;
      if (f.startsWith('cc-')) c[f] = v;
      else if (f) a[f] = a[f] || v;
    }
    const out = {};
    const num = (c['cc-number'] || '').replace(/\D/g, '');
    if (luhn(num)) {
      let exp = c['cc-exp'] || '';
      if (!exp && c['cc-month'] && c['cc-year']) exp = c['cc-month'].padStart(2, '0') + '/' + c['cc-year'].slice(-2);
      exp = exp.replace(/\D/g, '');
      exp = exp.length >= 4 ? exp.slice(0, 2) + '/' + exp.slice(-2) : '';
      out.card = { number: num, name: c['cc-name'] || '', exp };
    }
    if (a.street1 && (a.city || a.zip)) {
      if (!a.name && (a.first || a.last)) a.name = [a.first, a.last].filter(Boolean).join(' ');
      delete a.first; delete a.last; delete a.username;
      out.address = a;
    }
    const pws = passwordsIn(sc);
    if (pws.length && pws.some((p) => p.value)) {
      const u = userField(sc);
      send('suppa-login-submit', { user: u ? u.value.trim() : '' });
    }
    if (out.card || out.address) { lastSent = Date.now(); send('suppa-form-save', out); }
  }
  document.addEventListener('submit', (e) => { try { collect(e.target); } catch (x) {} }, true);
  // Many shops send with a script instead of a real form submit.
  document.addEventListener('click', (e) => {
    const b = e.target.closest && e.target.closest('button, input[type=submit], [role=button]');
    if (!b) return;
    const text = (b.textContent || b.value || '').toLowerCase();
    if (b.type === 'submit' || /pay|order|place|buy|checkout|continue|save|next|submit|sign|log ?in|register|weiter|bestellen|kaufen|оплат|заказ/.test(text)) {
      try { collect(scope(b)); } catch (x) {}
    }
  }, true);
})();
