// Suppa Browser - interface languages for Suppa's own pages (bar, menus,
// Settings). The English text in the pages is the key; scripts\lang.json has
// its Russian, German, Spanish and Chinese. Text added later by the page's
// script is translated as it appears. Elements with class "notr" (and code,
// inputs) are left alone. Edit lang.json to fix or add a translation.
//   SuppaI18n.set('ru')   language of this page ('en' = as written)
//   SuppaI18n.t('Save')   one text in the current language
(function () {
  if (window.SuppaI18n) return;
  let dict = null;
  let lang = 'en';
  let busy = false;
  const ATTRS = ['title', 'placeholder', 'aria-label'];
  const norm = (s) => s.replace(/\s+/g, ' ').trim();

  function load() {
    if (dict) return Promise.resolve(dict);
    return fetch('lang.json', { cache: 'no-store' }).then((r) => r.json()).then((j) => (dict = j)).catch(() => (dict = {}));
  }
  function t(s) {
    if (lang === 'en' || !dict || !dict[lang] || typeof s !== 'string') return s;
    const k = norm(s);
    const v = dict[lang][k];
    return v === undefined ? s : s.replace(k, v);
  }
  function skip(el) {
    return !el || (el.closest && el.closest('script, style, code, textarea, .notr, [contenteditable]'));
  }
  function textNode(n) {
    // A text the page changed itself counts as new English.
    if (n.__en === undefined || n.data !== n.__tr) n.__en = n.data;
    if (!norm(n.__en)) return;
    const v = t(n.__en);
    if (n.data !== v) n.data = v;
    n.__tr = n.data;
  }
  function attrs(el) {
    const en = el.__enA || (el.__enA = {});
    const tr = el.__trA || (el.__trA = {});
    for (const a of ATTRS) {
      if (!el.hasAttribute(a)) continue;
      const cur = el.getAttribute(a);
      if (en[a] === undefined || cur !== tr[a]) en[a] = cur;
      const v = t(en[a]);
      if (cur !== v) el.setAttribute(a, v);
      tr[a] = v;
    }
  }
  function walk(root) {
    if (!root) return;
    if (root.nodeType === 3) { if (!skip(root.parentElement)) textNode(root); return; }
    if (root.nodeType !== 1 || skip(root)) return;
    attrs(root);
    const w = document.createTreeWalker(root, NodeFilter.SHOW_ELEMENT | NodeFilter.SHOW_TEXT, {
      acceptNode: (n) => (n.nodeType === 1 && skip(n) ? NodeFilter.FILTER_REJECT : NodeFilter.FILTER_ACCEPT),
    });
    for (let n = w.nextNode(); n; n = w.nextNode()) {
      if (n.nodeType === 3) textNode(n); else attrs(n);
    }
  }
  function all() {
    busy = true;
    try {
      walk(document.body);
      document.title = t(document.title);
    } finally { busy = false; }
  }
  const obs = new MutationObserver((list) => {
    if (busy || lang === 'en' && !document.documentElement.__wasTranslated) return;
    busy = true;
    try {
      for (const m of list) {
        if (m.type === 'childList') m.addedNodes.forEach(walk);
        else if (m.type === 'characterData') { if (!skip(m.target.parentElement)) textNode(m.target); }
        else if (m.type === 'attributes' && !skip(m.target)) attrs(m.target);
      }
    } finally { busy = false; }
  });
  function start() {
    obs.observe(document.documentElement, { childList: true, subtree: true, characterData: true, attributes: true, attributeFilter: ATTRS });
  }
  if (document.documentElement) start(); else document.addEventListener('DOMContentLoaded', start);

  window.SuppaI18n = {
    set(l) {
      l = l || 'en';
      if (l === lang && (l === 'en' || dict)) return Promise.resolve();
      return load().then(() => {
        lang = dict && (dict[l] || l === 'en') ? l : 'en';
        if (lang !== 'en') document.documentElement.__wasTranslated = true;
        document.documentElement.lang = lang;
        if (document.body) all(); else document.addEventListener('DOMContentLoaded', all, { once: true });
      });
    },
    t,
    get lang() { return lang; },
  };
})();
