// Suppa Browser for Android - runs at the start of every web page (and frame).
// Talks to Suppa through window.suppaPage, which only answers for this page's
// own origin:
//   -> {t:'hello'}                       <- {t:'css', css}  site-specific ad hiding
//                                         <- {t:'pw_fill', items:[{user, pass}]}  saved passwords
//   -> {t:'pw_submit', user, pass}       a sign-in form was sent (Suppa offers to save it)
//   <- {t:'blob', url, token, name}      -> {t:'blob_data', token, data, name}  download of a blob: link
// Edit, then press Update in the browser.
(function () {
  if (window.__suppaPage || !window.suppaPage || /^suppa\./.test(location.hostname)) return;
  window.__suppaPage = true;
  var host = window.suppaPage;
  var send = function (m) { try { host.postMessage(JSON.stringify(m)); } catch (e) {} };

  function addStyle(css) {
    var s = document.createElement('style');
    s.textContent = css;
    (document.head || document.documentElement).appendChild(s);
  }

  // ---- passwords: fill in, and offer to save after signing in ----
  var creds = [];
  function visible(el) { return !!(el.offsetWidth || el.offsetHeight || el.getClientRects().length); }
  function userFieldFor(pw) {
    var list = Array.prototype.slice.call((pw.form || document).querySelectorAll('input'));
    var i = list.indexOf(pw);
    for (var k = i - 1; k >= 0; k--) {
      var t = (list[k].type || 'text').toLowerCase();
      if ((t === 'text' || t === 'email' || t === 'tel') && visible(list[k])) return list[k];
    }
    return null;
  }
  function setValue(el, v) {
    var proto = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value');
    proto.set.call(el, v);
    el.dispatchEvent(new Event('input', { bubbles: true }));
    el.dispatchEvent(new Event('change', { bubbles: true }));
  }
  function fill() {
    if (!creds.length) return;
    var c = creds[0];
    document.querySelectorAll('input[type=password]').forEach(function (pw) {
      if (!visible(pw) || pw.value || pw.autocomplete === 'new-password') return;
      var u = userFieldFor(pw);
      if (u && !u.value && c.user) setValue(u, c.user);
      setValue(pw, c.pass);
    });
  }
  var lastSent = '';
  function capture(scope) {
    var pws = (scope || document).querySelectorAll('input[type=password]');
    for (var i = 0; i < pws.length; i++) {
      var pw = pws[i];
      if (!pw.value) continue;
      var u = userFieldFor(pw);
      var user = u ? u.value : '';
      var key = user + '\n' + pw.value;
      if (key === lastSent) return;
      lastSent = key;
      send({ t: 'pw_submit', user: user, pass: pw.value });
      return;
    }
  }
  document.addEventListener('submit', function (e) { capture(e.target); }, true);
  // Sign-in pages that never send a real form: the button next to the password.
  document.addEventListener('click', function (e) {
    var b = e.target.closest && e.target.closest('button, input[type=submit], [role=button]');
    if (b && document.querySelector('input[type=password]')) setTimeout(function () { capture(); }, 0);
  }, true);
  document.addEventListener('keydown', function (e) {
    if (e.key === 'Enter' && e.target && e.target.type === 'password') capture();
  }, true);

  // ---- downloads of blob: links ----
  function blobDownload(m) {
    fetch(m.url).then(function (r) { return r.blob(); }).then(function (b) {
      var fr = new FileReader();
      fr.onload = function () { send({ t: 'blob_data', token: m.token, data: fr.result, name: m.name }); };
      fr.readAsDataURL(b);
    }).catch(function () { send({ t: 'blob_data', token: m.token, data: '', name: m.name }); });
  }

  host.addEventListener('message', function (e) {
    var m;
    try { m = JSON.parse(e.data); } catch (x) { return; }
    if (m.t === 'css') {
      if (document.documentElement) addStyle(m.css);
      else document.addEventListener('DOMContentLoaded', function () { addStyle(m.css); });
    } else if (m.t === 'pw_fill') {
      creds = m.items || [];
      fill();
      document.addEventListener('DOMContentLoaded', fill);
      // Sign-in forms drawn later by the page's own scripts.
      var n = 0;
      var t = setInterval(function () { fill(); if (++n > 20) clearInterval(t); }, 500);
    } else if (m.t === 'blob') {
      blobDownload(m);
    }
  });
  send({ t: 'hello' });
})();
