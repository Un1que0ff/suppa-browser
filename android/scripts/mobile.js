// Suppa Browser for Android - the phone parts of the bar, after ui.js (same page,
// same messages). Edit, then press "Update" in the browser.
//
// Extra messages to the browser:   tabs  tabs_menu {x, y}  hole {top, bottom}
//                                  find {op: text|next|prev|close, text}
// Extra messages from the browser: progress {p}  find {open}  find_result {i, n}
//                                  sync {..., count, progress}
(function () {
  'use strict';
  const row2 = $('row2');

  // ---- tabs button: opens the tabs screen; long press = new tab / incognito / close ----
  const tb = document.createElement('button');
  tb.id = 'tabsbtn';
  tb.className = 'nav';
  tb.title = 'Tabs';
  tb.innerHTML = '<span class="count">1</span>';
  row2.insertBefore(tb, $('menu'));
  tb.onclick = () => post({ cmd: 'tabs' });
  tb.addEventListener('contextmenu', (e) => {
    e.preventDefault();
    const r = tb.getBoundingClientRect();
    post({ cmd: 'tabs_menu', x: r.right, y: r.bottom + 4 });
  });

  // ---- loading line ----
  const pr = document.createElement('div');
  pr.id = 'progress';
  row2.appendChild(pr);
  let prT = 0;
  function progress(p) {
    clearTimeout(prT);
    if (p >= 100) {
      pr.style.width = '100%';
      prT = setTimeout(() => { pr.classList.remove('on'); setTimeout(() => { pr.style.width = '0'; }, 300); }, 200);
    } else {
      pr.classList.add('on');
      pr.style.width = Math.max(8, p) + '%';
    }
  }

  // ---- find in page ----
  const fr = document.createElement('div');
  fr.id = 'findrow';
  fr.innerHTML = '<input id="findq" placeholder="Find in page" spellcheck="false" autocomplete="off"><span class="n" id="findn"></span>' +
    '<button class="nav" id="findp" title="Previous"><svg viewBox="0 0 16 16"><path d="M3.5 10 8 5.5l4.5 4.5"/></svg></button>' +
    '<button class="nav" id="findx" title="Next"><svg viewBox="0 0 16 16"><path d="M3.5 6 8 10.5 12.5 6"/></svg></button>' +
    '<button class="nav" id="findc" title="Close"><svg viewBox="0 0 16 16"><path d="M4 4l8 8M12 4l-8 8"/></svg></button>';
  row2.appendChild(fr);
  const fq = $('findq');
  let fT = 0;
  fq.addEventListener('input', () => { clearTimeout(fT); fT = setTimeout(() => post({ cmd: 'find', op: 'text', text: fq.value }), 120); });
  fq.addEventListener('keydown', (e) => {
    if (e.key === 'Enter') post({ cmd: 'find', op: e.shiftKey ? 'prev' : 'next' });
    else if (e.key === 'Escape') post({ cmd: 'find', op: 'close' });
  });
  $('findp').onclick = () => post({ cmd: 'find', op: 'prev' });
  $('findx').onclick = () => post({ cmd: 'find', op: 'next' });
  $('findc').onclick = () => post({ cmd: 'find', op: 'close' });

  // ---- typing an address: the field gets the row ----
  const addrEl = $('addr');
  addrEl.setAttribute('inputmode', 'url');
  addrEl.setAttribute('enterkeyhint', 'go');
  addrEl.addEventListener('focus', () => document.body.classList.add('addr-focus'));
  addrEl.addEventListener('blur', () => document.body.classList.remove('addr-focus'));

  // ---- where the page goes: the rectangle ui.js leaves free (#hole) ----
  let lastHole = '';
  function report() {
    const h = $('hole');
    const top = parseFloat(h.style.top) || 0, bottom = parseFloat(h.style.bottom) || 0;
    const key = top + ',' + bottom;
    if (key === lastHole) return;
    lastHole = key;
    post({ cmd: 'hole', top, bottom });
  }
  const baseApply = applyRows;
  applyRows = function () { baseApply(); report(); };

  // The interface scale (Settings -> Appearance) is the page zoom of the bar.
  function setScale(s) {
    let vp = document.querySelector('meta[name=viewport]');
    if (!vp) { vp = document.createElement('meta'); vp.name = 'viewport'; document.head.appendChild(vp); }
    vp.content = 'width=device-width, initial-scale=' + s + ', minimum-scale=' + s + ', maximum-scale=' + s + ', user-scalable=no';
  }

  window.chrome.webview.addEventListener('message', (e) => {
    const m = e.data;
    if (m.type === 'init') {
      if (m.scale) setScale(m.scale);
      document.body.classList.toggle('nav-bottom', !!(m.pos && m.pos.nav === 'bottom'));
      lastHole = '';
      report();
    } else if (m.type === 'sync') {
      tb.querySelector('.count').textContent = m.count > 99 ? '∞' : String(m.count || 0);
      tb.title = (m.count || 0) + (m.inc ? ' incognito tabs' : ' tabs');
      if (typeof m.progress === 'number') progress(m.progress);
    } else if (m.type === 'progress') {
      progress(m.p);
    } else if (m.type === 'find') {
      document.body.classList.toggle('finding', !!m.open);
      if (m.open) { fq.value = ''; $('findn').textContent = ''; fq.focus(); } else fq.blur();
    } else if (m.type === 'find_result') {
      $('findn').textContent = m.n ? m.i + '/' + m.n : (fq.value ? '0/0' : '');
    }
  });
})();
