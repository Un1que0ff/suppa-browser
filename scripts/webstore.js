// Suppa Browser - Chrome Web Store helper (runs only on chromewebstore.google.com).
// The store's "Add to Chrome" button would start Edge's built-in installer, which
// can't install in Suppa ("Download interrupted"). This script relabels the
// button "Add to Suppa" and hands the install to Suppa instead.
// Edit, then press Update in the browser.
(function () {
  if (location.hostname !== 'chromewebstore.google.com' || window.top !== window) return;
  if (window.__suppaStore) return;
  window.__suppaStore = true;

  // "Add to Chrome" in several languages.
  var ADD = /(add to|pridėti|hinzufügen|ajouter|añadir|agregar|aggiungi|adicionar|добавить|установить|dodaj|toevoegen|lägg till|lisää|přidat|ekle)/i;
  var BROWSER = /(chrome|edge)/i;

  function extId() {
    var m = location.pathname.match(/\/([a-p]{32})(?:[/?#]|$)/);
    return m ? m[1] : '';
  }

  function extName() {
    var h = document.querySelector('main h1, h1');
    var n = h && h.textContent.trim();
    if (!n) n = (document.title || '').replace(/\s*[-|–]\s*Chrome Web Store.*$/i, '').trim();
    return n.slice(0, 80);
  }

  function isAddButton(b) {
    if (!b) return false;
    var t = (b.innerText || b.textContent || '').trim();
    return t.length < 60 && ADD.test(t) && (BROWSER.test(t) || /suppa/i.test(t));
  }

  // Click on "Add to Chrome" -> ask Suppa (it shows its own "Add to Suppa?" popup).
  document.addEventListener('click', function (e) {
    var b = e.target.closest && e.target.closest('button, [role="button"]');
    if (!isAddButton(b)) return;
    var id = extId();
    if (!id) return;
    e.preventDefault();
    e.stopImmediatePropagation();
    var r = b.getBoundingClientRect();
    window.chrome.webview.postMessage('suppa-ext-install:' + JSON.stringify({
      id: id, name: extName(), x: Math.round(r.left), y: Math.round(r.bottom + 6)
    }));
  }, true);

  // Relabel the button: "Add to Chrome" -> "Add to Suppa".
  function relabel() {
    document.querySelectorAll('button, [role="button"]').forEach(function (b) {
      if (!isAddButton(b)) return;
      var walker = document.createTreeWalker(b, NodeFilter.SHOW_TEXT);
      for (var n = walker.nextNode(); n; n = walker.nextNode()) {
        if (BROWSER.test(n.nodeValue)) n.nodeValue = n.nodeValue.replace(/(Microsoft Edge|Chrome|Edge)/gi, 'Suppa');
      }
    });
  }
  var pending = false;
  // Observe `document` (this script runs before <html> exists).
  new MutationObserver(function () {
    if (pending) return;
    pending = true;
    setTimeout(function () { pending = false; relabel(); }, 200);
  }).observe(document, { childList: true, subtree: true, characterData: true });
  document.addEventListener('DOMContentLoaded', relabel);
})();
