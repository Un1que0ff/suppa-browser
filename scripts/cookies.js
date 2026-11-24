// Suppa Browser - auto-accept cookie banners.
// Runs on every page (turn off in Settings). Clicks the "Accept all" button of
// common cookie/consent popups. Edit, then press Update in the browser.
(function () {
  if (window.__suppaCookies) return;
  window.__suppaCookies = true;

  // Known consent tools: exact buttons (checked first).
  var SELECTORS = [
    '#onetrust-accept-btn-handler', '#accept-recommended-btn-handler',
    '#CybotCookiebotDialogBodyLevelButtonLevelOptinAllowAll', '#CybotCookiebotDialogBodyButtonAccept',
    '#didomi-notice-agree-button', '.qc-cmp2-summary-buttons button[mode="primary"]',
    '#truste-consent-button', '.fc-cta-consent', 'button[data-testid="uc-accept-all-button"]',
    '#L2AGLb', 'button.sp_choice_type_11', '[data-cookiefirst-action="accept"]', '.cky-btn-accept',
    '#cookie_action_close_header', '.cc-allow', '.cc-accept-all', '#acceptAllCookies',
    '#accept-all-cookies', '#cookiescript_accept', '.iubenda-cs-accept-btn', '.osano-cm-accept-all',
    '#hs-eu-confirmation-button', '.cmpboxbtnyes', '#gdpr-cookie-accept', '.moove-gdpr-infobar-allow-all',
    '.cookie-consent-accept-all', '[data-action="accept-all"]', '[data-qa="accept-all"]',
    'button[aria-label="Accept all"]', 'button[aria-label="Accept all cookies"]', 'button[aria-label="Allow all"]'
  ];

  // Button texts (many languages, incl. Lithuanian).
  var WORDS = /^(accept all( cookies)?|allow all( cookies)?|accept( cookies)?|i accept|agree|i agree|agree and (close|continue)|accept and (close|continue)|got it|ok(ay)?|yes,? i agree|continue with all cookies|alle akzeptieren|alle cookies akzeptieren|akzeptieren|zustimmen|tout accepter|accepter( et fermer)?|j'accepte|aceptar( todo| todas)?|accetta( tutto| tutti)?|sutinku( su visais)?|priimti( visus| visus slapukus| slapukus)?|leisti visus|patvirtinti visus|принять( все)?|согласен|я согласен|zgadzam się|akceptuję( wszystkie)?|alles accepteren|accepteren|godkänn alla|acceptera alla|hyväksy kaikki|aceitar( tudo| todos)?|souhlasím|přijmout vše|kabul et)$/i;
  // Only buttons inside something that looks like a cookie notice.
  var CONTEXT = /cookie|consent|gdpr|privacy|privacidad|datenschutz|confidentialit|slapuk|privatum|cmp|tracking/i;

  var done = false;
  var started = Date.now();

  function clean(s) { return (s || '').replace(/\s+/g, ' ').trim().replace(/[.!]$/, ''); }

  function inContext(el) {
    if (/^consent\./.test(location.hostname)) return true;
    for (var n = el, i = 0; n && i < 10; n = n.parentElement, i++) {
      var tag = (n.id || '') + ' ' + (typeof n.className === 'string' ? n.className : '') + ' ' + (n.getAttribute && (n.getAttribute('aria-label') || '') || '');
      if (CONTEXT.test(tag)) return true;
    }
    var box = el.closest('[role="dialog"],dialog,[aria-modal="true"],aside,footer,form,div');
    return !!(box && box.textContent && box.textContent.length < 4000 && CONTEXT.test(box.textContent));
  }

  function click(el) {
    try { el.click(); done = true; } catch (e) {}
  }

  function scan() {
    if (done || !document.body) return;
    for (var i = 0; i < SELECTORS.length; i++) {
      var el = document.querySelector(SELECTORS[i]);
      if (el) { click(el); return; }
    }
    var cands = document.querySelectorAll('button, [role="button"], a[href="#"], a:not([href]), input[type="button"], input[type="submit"]');
    for (var j = 0; j < cands.length && j < 400; j++) {
      var b = cands[j];
      var text = clean(b.value || b.textContent || b.getAttribute('aria-label'));
      if (text.length > 40 || !WORDS.test(text)) continue;
      if (!inContext(b)) continue;
      click(b);
      return;
    }
  }

  var timer = 0;
  function later() {
    if (done || timer) return;
    timer = setTimeout(function () { timer = 0; scan(); }, 250);
  }

  function start() {
    scan();
    if (done) return;
    var obs = new MutationObserver(function () {
      if (done || Date.now() - started > 20000) { obs.disconnect(); return; }
      later();
    });
    obs.observe(document.documentElement, { childList: true, subtree: true });
    setTimeout(scan, 1500);
    setTimeout(scan, 4000);
  }

  if (document.readyState === 'loading') document.addEventListener('DOMContentLoaded', start);
  else start();
})();
