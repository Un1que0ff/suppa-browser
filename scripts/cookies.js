// Suppa Browser - answers cookie banners for you.
// Runs on every page (Settings -> Privacy -> Cookie popups):
//   "necessary" (default): clicks "Reject all" / "Only necessary"; if the banner
//                has no such button, its close (×) button (= no consent).
//   "all":       clicks "Accept all".
// Suppa puts window.__suppaCookieMode in front of this file.
// Edit, then press Update in the browser.
(function () {
  if (window.__suppaCookies) return;
  window.__suppaCookies = true;
  var MODE = window.__suppaCookieMode === 'all' ? 'all' : 'necessary';

  // "Reject" / "only necessary" buttons of known consent tools.
  var REJECT_SELECTORS = [
    '#onetrust-reject-all-handler', '.ot-pc-refuse-all-handler', '#CybotCookiebotDialogBodyButtonDecline',
    '#CybotCookiebotDialogBodyLevelButtonLevelOptinDeclineAll', '#didomi-notice-disagree-button',
    '.didomi-continue-without-agreeing', 'button[data-testid="uc-deny-all-button"]', '#W0wltc',
    'button.sp_choice_type_13', '.fc-cta-do-not-consent', '[data-cookiefirst-action="reject"]', '.cky-btn-reject',
    '.cc-deny', '.cc-dismiss-necessary', '#rejectAllCookies', '#reject-all-cookies', '#cookiescript_reject',
    '.iubenda-cs-reject-btn', '.osano-cm-denyAll', '#hs-eu-decline-button', '.cmpboxbtnno', '#gdpr-cookie-reject',
    '.moove-gdpr-infobar-reject-btn', '.cookie-consent-reject-all', '[data-action="reject-all"]', '[data-qa="reject-all"]',
    'button[aria-label="Reject all"]', 'button[aria-label="Reject all cookies"]', 'button[aria-label="Decline all"]',
    'button[aria-label="Only necessary"]', '#truste-consent-required', '.qc-cmp2-summary-buttons button[mode="secondary"]:first-child'
  ];
  var REJECT_WORDS = /^(reject( all)?( cookies)?|decline( all)?( cookies)?|deny( all)?|refuse( all)?|disagree|i disagree|do not (accept|agree|consent)|don'?t (accept|allow)|no,? thanks|(use )?(only|strictly) (necessary|essential|required)( cookies)?( only)?|(necessary|essential|required) (cookies )?only|accept (only )?(necessary|essential|required)( cookies)?( only)?|allow (only )?(necessary|essential|required)( cookies)?|continue without (accepting|agreeing)|alle ablehnen|ablehnen|nur (notwendige|erforderliche|essenzielle)( cookies)?( akzeptieren| zulassen)?|tout refuser|refuser( tout)?|continuer sans accepter|rechazar( todo| todas)?|solo (necesarias|esenciales)|rifiuta( tutto| tutti)?|solo necessari|atmesti( visus)?|tik (būtini|būtinieji)|отклонить( все)?|только (необходимые|обязательные)|odrzuć( wszystkie)?|tylko niezbędne|alles weigeren|weigeren|alleen noodzakelijk|avvisa alla|endast nödvändiga|hylkää kaikki|rejeitar( tudo| todos)?|apenas necessários|odmítnout vše|pouze nezbytné|reddet)$/i;
  var CLOSE_SELECTORS = 'button[aria-label*="close" i], button[aria-label*="dismiss" i], button[title*="close" i], [class*="close" i][role="button"], button[class*="close" i]';
  var closeTries = 0;

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

  function shown(el) {
    var r = el.getBoundingClientRect();
    return r.width > 0 && r.height > 0;
  }

  function scan() {
    if (done || !document.body) return;
    var sels = MODE === 'all' ? SELECTORS : REJECT_SELECTORS;
    var words = MODE === 'all' ? WORDS : REJECT_WORDS;
    for (var i = 0; i < sels.length; i++) {
      var el = document.querySelector(sels[i]);
      if (el && shown(el)) { click(el); return; }
    }
    var cands = document.querySelectorAll('button, [role="button"], a[href="#"], a:not([href]), input[type="button"], input[type="submit"]');
    var bannerButton = null;
    for (var j = 0; j < cands.length && j < 400; j++) {
      var b = cands[j];
      var text = clean(b.value || b.textContent || b.getAttribute('aria-label'));
      if (text.length > 60) continue;
      if (words.test(text)) {
        if (!inContext(b)) continue;
        click(b);
        return;
      }
      if (!bannerButton && WORDS.test(text) && inContext(b) && shown(b)) bannerButton = b;
    }
    // Only necessary, but the banner has just "Accept": its × (no consent given).
    if (MODE === 'necessary' && bannerButton && ++closeTries >= 2) {
      var box = bannerButton.closest('[role="dialog"],dialog,[aria-modal="true"],aside,div');
      for (var k = 0; box && k < 4; k++, box = box.parentElement && box.parentElement.closest('div,aside,[role="dialog"]')) {
        var x = box.querySelector(CLOSE_SELECTORS);
        if (x && shown(x)) { click(x); return; }
      }
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
