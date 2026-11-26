// Suppa Browser - top bar logic. Edit, then press "Update" in the browser.
//
// Messages to the browser:   post({cmd: ...})
//   navigate {text}  activate {index}  close {index}  run {name}
//   menu {which, x, y}  tab_menu {index, x, y}  set_theme {dark, bg, border}
//   accent {color}  focus_page  ready  drag {edge}
//   saved_open {path, how}  saved_folder {path, x, y}  saved_menu {path, x, y}
//   saved_move {path, kind, target}  saved_rename {path, title}
// Messages from the browser (event "message"):
//   init {dark, accent, saved}
//   sync {tabs:[{t,s,i,f}], active, url, back, fwd, max, inc}
//   update {available, building}   focus_address
//   saved {items:[{t,u,i} | {f,n}]}   rename {path}
'use strict';

const $ = (id) => document.getElementById(id);
const post = (msg) => window.chrome.webview.postMessage(msg);

// Selected-tab accent colour cycles like the tabs in Aviatema Tools
// (Settings -> Accent colour: rainbow = cycle, or one fixed colour).
// [dark theme, light theme]
const ACCENTS = [
  ['#b07cff', '#7048e8'], // violet
  ['#4cd37b', '#2f9e44'], // green
  ['#3cd6e0', '#0c8599'], // cyan
  ['#ffd43b', '#f08c00'], // yellow
  ['#ff8a8a', '#ec6a6a'], // red
  ['#5b9dff', '#2f6fed'], // blue
  ['#ff9a3c', '#e8590c'], // orange
];
const TAB_MAX = 220, TAB_MIN = 48, TAB_GAP = 3;

let dark = true;
let accent = 'cycle';
let lastAccent = '';
let state = { tabs: [], active: 0, url: '', back: false, fwd: false, max: false, inc: false };

// Text on accent backgrounds: Settings -> Text on accent (auto / black / white).
let onAccent = 'auto';
function onColor(hex) {
  if (onAccent === 'black') return '#1c1c1c';
  if (onAccent === 'white') return '#ffffff';
  const n = parseInt(hex.slice(1), 16);
  const r = n >> 16, g = (n >> 8) & 255, b = n & 255;
  return 0.299 * r + 0.587 * g + 0.114 * b > 160 ? '#1c1c1c' : '#ffffff';
}

function applyAccent() {
  const a = accent === 'cycle' ? ACCENTS[state.active % ACCENTS.length][dark ? 0 : 1] : accent;
  document.body.style.setProperty('--accent', a);
  document.body.style.setProperty('--on-accent', onColor(a));
  if (a !== lastAccent) { lastAccent = a; post({ cmd: 'accent', color: a }); }
}

// Settings -> Pro settings -> Colors: custom colours on top of the theme.
const COLOR_NAMES = ['bg', 'panel', 'field', 'btn', 'btn-hover', 'tab', 'tab-fg', 'fg', 'muted', 'border', 'incognito'];
function applyColors(c) {
  for (const n of COLOR_NAMES) {
    if (c && c[n]) document.body.style.setProperty('--' + n, c[n]);
    else document.body.style.removeProperty('--' + n);
  }
}

function setTheme(isDark, notify) {
  dark = isDark;
  const was = document.body.classList.contains('light');
  document.body.classList.toggle('light', !dark);
  if (was !== !dark) retoneIcons();
  applyAccent();
  if (notify) {
    const cs = getComputedStyle(document.body);
    post({ cmd: 'set_theme', dark, bg: cs.getPropertyValue('--bg').trim(), border: cs.getPropertyValue('--border').trim() });
  }
}

// ---------- tabs ----------
const INC_ICON = '<svg class="inc-ico" viewBox="0 0 16 16" fill="currentColor"><path d="M2 7h12l-1.2-3.6A1 1 0 0 0 11.8 3H4.2a1 1 0 0 0-.95.7z"/><circle cx="4.5" cy="11" r="2.3" fill="none" stroke="currentColor" stroke-width="1.4"/><circle cx="11.5" cy="11" r="2.3" fill="none" stroke="currentColor" stroke-width="1.4"/><path d="M6.8 10.6h2.4" stroke="currentColor" stroke-width="1.2"/></svg>';
let tabEls = [];            // live tab elements in order (kept per tab id, never rebuilt)
const byId = new Map();     // tab id -> element
let frozenW = 0;            // width kept while closing tabs with the mouse (like Chrome)
const ANIM_MS = 180;        // open / close / slide animation length (see .tab transition in ui.css)

function makeTab() {
  const el = document.createElement('div');
  el.className = 'tab';
  el._ico = document.createElement('span');
  el._ico.style.display = 'contents';
  el._title = document.createElement('span');
  el._title.className = 'title';
  el._x = document.createElement('span');
  el._x.className = 'x';
  el._x.textContent = '×';
  el._x.title = 'Close tab (Ctrl+W)';
  el.append(el._ico, el._title, el._x);
  const closeIt = () => {
    frozenW = el.getBoundingClientRect().width;   // keep widths so the next × is under the mouse
    post({ cmd: 'close', index: el._i });
  };
  el.addEventListener('mousedown', (e) => {
    if (e.button === 1) e.preventDefault();
  });
  el.addEventListener('pointerdown', (e) => {
    if (e.button !== 0 || e.target === el._x || el.classList.contains('closing')) return;
    post({ cmd: 'activate', index: el._i });
    startTabDrag(e, el);
  });
  el.addEventListener('auxclick', (e) => { if (e.button === 1) closeIt(); });
  el._x.addEventListener('click', (e) => { e.stopPropagation(); closeIt(); });
  el.addEventListener('contextmenu', (e) => {
    e.preventDefault();
    post({ cmd: 'tab_menu', index: el._i, x: e.clientX, y: e.clientY });
  });
  return el;
}

function setIcon(el, t) {
  const key = t.s === 'loading' ? 'spin' : t.i ? 'inc' : (t.f || '');
  if (el._icoKey === key) return;
  el._icoKey = key;
  el._ico.textContent = '';
  if (key === 'spin') {
    const sp = document.createElement('span');
    sp.className = 'spin';
    el._ico.appendChild(sp);
  } else if (key === 'inc') {
    el._ico.innerHTML = INC_ICON;
  } else if (key) {
    const img = document.createElement('img');
    img.className = 'fav';
    img.src = key;
    img.onerror = () => img.remove();
    toneIcon(img, key);
    el._ico.appendChild(img);
  }
}

// Site icons in Suppa's theme (light / dark), not the system's:
//  * An SVG icon that changes colour by itself (Rust's: white on dark systems)
//    follows the system setting, so it is shown as a copy set to Suppa's theme.
//  * An icon of one plain colour on a see-through background (GitHub's white
//    cat, a black logo) gets class "ic-w" (white) or "ic-b" (black); the page
//    inverts "ic-w" in the light theme and "ic-b" in the dark one.
//  * Coloured icons and icons with their own background are left alone.
// Only icons whose site allows reading them can be checked (most icon servers do).
const ICON_TONE = new Map();
function iconTone(url) {
  return new Promise((done) => {
    const probe = new Image();
    probe.onerror = () => done('');
    probe.onload = () => {
      try {
        const c = document.createElement('canvas');
        c.width = c.height = 24;
        const g = c.getContext('2d');
        g.drawImage(probe, 0, 0, 24, 24);
        const d = g.getImageData(0, 0, 24, 24).data;
        let clear = 0, solid = 0, white = 0, black = 0, colour = 0;
        for (let i = 0; i < d.length; i += 4) {
          if (d[i + 3] < 60) { clear++; continue; }
          solid++;
          const l = 0.299 * d[i] + 0.587 * d[i + 1] + 0.114 * d[i + 2];
          if (Math.max(d[i], d[i + 1], d[i + 2]) - Math.min(d[i], d[i + 1], d[i + 2]) > 40) colour++;
          else if (l > 200) white++;
          else if (l < 60) black++;
        }
        const see = clear / (clear + solid) > 0.2 && colour < solid * 0.08;
        done(!see || !solid ? '' : white > solid * 0.85 ? 'w' : black > solid * 0.85 ? 'b' : '');
      } catch (e) { done(''); }
    };
    probe.src = url;
  });
}
// -> { tone: '' | 'w' | 'b', src: address to show (a themed copy for adaptive SVGs) }
async function checkIcon(src, light) {
  try {
    const r = await fetch(src, { mode: 'cors', credentials: 'omit' });
    if (!r.ok) return { tone: '', src };
    const blob = await r.blob();
    if (/svg/.test(blob.type) || /\.svg(\?|$)/i.test(src)) {
      const text = await blob.text();
      if (/prefers-color-scheme/i.test(text)) {
        // Set the icon's own light/dark choice to Suppa's theme.
        const yes = 'min-width: 0px', no = 'max-width: 0px) and (min-width: 1px';
        const fixed = text
          .replace(/prefers-color-scheme\s*:\s*dark/gi, light ? no : yes)
          .replace(/prefers-color-scheme\s*:\s*light/gi, light ? yes : no);
        return { tone: '', src: URL.createObjectURL(new Blob([fixed], { type: 'image/svg+xml' })) };
      }
    }
    return { tone: await iconTone(URL.createObjectURL(blob)), src };
  } catch (e) {
    return { tone: '', src };   // the site does not allow reading its icon
  }
}
function toneIcon(img, src) {
  if (!/^https?:/.test(src)) return;
  img._tone = src;
  const light = document.body.classList.contains('light');
  const key = src + (light ? ' light' : ' dark');
  img._key = key;
  // only the answer for the current theme counts (an older check may finish later)
  const set = (v) => {
    if (img._key !== key || !v) return;
    img.classList.toggle('ic-w', v.tone === 'w');
    img.classList.toggle('ic-b', v.tone === 'b');
    if (img.getAttribute('src') !== v.src) img.src = v.src;
  };
  let t = ICON_TONE.get(key);
  if (t === undefined) {
    t = checkIcon(src, light);
    ICON_TONE.set(key, t);
    t.then((v) => ICON_TONE.set(key, v));
  }
  if (t instanceof Promise) t.then(set); else set(t);
}
// The theme changed: check the shown icons again (after the new colours apply).
function retoneIcons() {
  setTimeout(() => document.querySelectorAll('img').forEach((i) => { if (i._tone) toneIcon(i, i._tone); }), 60);
}

// ---------- tab groups: a coloured label in front of the group's tabs ----------
const chipById = new Map();
function groupChip(g, count) {
  let chip = chipById.get(g.id);
  if (!chip) {
    chip = document.createElement('div');
    chip.className = 'gchip';
    chip._name = document.createElement('span');
    chip._name.className = 'gname';
    chip._count = document.createElement('span');
    chip._count.className = 'gcount';
    chip.append(chip._name, chip._count);
    chip.addEventListener('click', () => { if (!chip.querySelector('input')) post({ cmd: 'group_toggle', id: chip._gid }); });
    chip.addEventListener('contextmenu', (e) => { e.preventDefault(); post({ cmd: 'group_menu', id: chip._gid, x: e.clientX, y: e.clientY }); });
    chip.addEventListener('mousedown', (e) => e.stopPropagation());   // not a window drag
    chipById.set(g.id, chip);
  }
  chip._gid = g.id;
  chip.style.setProperty('--g', g.color);
  chip.style.color = onColor(g.color);
  chip.classList.toggle('collapsed', !!g.collapsed);
  chip.classList.toggle('noname', !g.name);
  if (!chip.querySelector('input')) chip._name.textContent = g.name;
  chip._count.textContent = g.collapsed || !g.name ? String(count) : '';
  chip.title = (g.name || 'Group') + ' - ' + count + ' tab' + (count === 1 ? '' : 's') + (g.collapsed ? ' (click to expand)' : ' (click to collapse)');
  return chip;
}
// A new group asks for its name; the request can arrive before its label is drawn.
let pendingGroupRename = 0;
function startGroupRename(id) {
  const chip = chipById.get(id);
  if (!chip || !chip.isConnected) { pendingGroupRename = id; return; }
  if (chip.querySelector('input')) return;
  const inp = document.createElement('input');
  inp.className = 'rename';
  inp.placeholder = 'Group name';
  inp.value = chip._name.textContent;
  inp.spellcheck = false;
  chip._name.textContent = '';
  chip._name.appendChild(inp);
  inp.focus();
  inp.select();
  const started = Date.now();
  let done = false;
  const finish = (ok) => {
    if (done) return;
    done = true;
    const v = inp.value.trim();
    inp.remove();
    if (ok) {
      // Shown right away (the browser confirms with the next sync).
      const g = (state.groups || []).find((x) => x.id === id);
      if (g) g.name = v;
      post({ cmd: 'group_rename', id, name: v });
    }
    renderTabs();
  };
  inp.addEventListener('keydown', (e) => { if (e.key === 'Enter') finish(true); else if (e.key === 'Escape') finish(false); e.stopPropagation(); });
  inp.addEventListener('blur', () => { if (Date.now() - started < 600) { setTimeout(() => inp.focus(), 50); return; } finish(true); });
  inp.addEventListener('click', (e) => e.stopPropagation());
  inp.addEventListener('pointerdown', (e) => e.stopPropagation());
}

// Closed tab: shrinks to nothing, then is removed.
function removeAnimated(el) {
  el.classList.add('closing');
  el.style.width = '0px';
  setTimeout(() => el.remove(), ANIM_MS + 40);
}

function renderTabs() {
  if (tabDrag && tabDrag.on) return;            // finish the drag first (sync comes again after it)
  const box = $('tabs');
  const plus = $('newtab');
  const ids = new Set(state.tabs.map((t) => t.id));
  for (const [id, el] of byId) {
    if (!ids.has(id)) { byId.delete(id); removeAnimated(el); }
  }
  const order = [];
  const seq = [];                                 // tabs + group labels, in row order
  const fresh = [];
  const groups = new Map((state.groups || []).map((g) => [g.id, g]));
  for (const [gid, chip] of chipById) {
    if (!groups.has(gid)) { chipById.delete(gid); chip.remove(); }
  }
  state.tabs.forEach((t, i) => {
    let el = byId.get(t.id);
    if (!el) {
      el = makeTab();
      el.style.width = '0px';                     // new tab grows in
      byId.set(t.id, el);
      fresh.push(el);
    }
    if (t.g && groups.has(t.g) && (i === 0 || state.tabs[i - 1].g !== t.g)) seq.push(groupChip(groups.get(t.g), state.tabs.filter((x) => x.g === t.g).length));
    order.push(el);
    seq.push(el);
  });
  // Put elements in tab order, right before "+". Tabs that are closing stay
  // where they are (skipped here), so they shrink away in place.
  const nextLive = (el) => {
    let n = el.nextSibling;
    while (n && n.classList && n.classList.contains('closing')) n = n.nextSibling;
    return n;
  };
  let ref = plus;
  for (let k = seq.length - 1; k >= 0; k--) {
    if (seq[k].parentNode !== box || nextLive(seq[k]) !== ref) box.insertBefore(seq[k], ref);
    ref = seq[k];
  }
  tabEls = order;
  if (fresh.length) void box.offsetWidth;        // start the grow animation from width 0
  state.tabs.forEach((t, i) => {
    const el = tabEls[i];
    el._i = i;
    const g = t.g && groups.get(t.g);
    const cls = 'tab' + (i === state.active ? ' active' : '') + (t.s ? ' ' + t.s : '') + (t.i ? ' inc' : '')
      + (g ? ' grouped' : '') + (g && g.collapsed ? ' gcollapsed' : '');
    if (el.className !== cls) el.className = cls;
    if (g) {
      el.style.setProperty('--g', g.color);
      el.style.setProperty('--g-fg', onColor(g.color));     // text on the group colour
    } else {
      el.style.removeProperty('--g');
      el.style.removeProperty('--g-fg');
    }
    if (el._title.textContent !== t.t) el._title.textContent = t.t;
    const tip = t.i ? t.t + ' (incognito)' : t.t;
    if (el.title !== tip) el.title = tip;
    setIcon(el, t);
  });
  layoutTabs();
  applyAccent();
  if (pendingGroupRename && chipById.has(pendingGroupRename)) {
    const id = pendingGroupRename;
    pendingGroupRename = 0;
    startGroupRename(id);
  }
}

// Chrome-like widths: every tab is TAB_MAX wide until the row is full, then
// all shrink evenly. "+" always sits right after the last tab.
// Direction tabs / saved tabs run in: across (row at the top/bottom) or down (in a column).
const inCol = (k) => !!$(k === 'tabs' ? 'row1' : k === 'nav' ? 'row2' : 'row3').closest('.col');
function AX(k) {
  return inCol(k)
    ? { pos: 'clientY', start: 'top', end: 'bottom', size: 'height', tr: 'translateY' }
    : { pos: 'clientX', start: 'left', end: 'right', size: 'width', tr: 'translateX' };
}

function layoutTabs() {
  const n = tabEls.length;
  if (!n) return;
  if (inCol('tabs')) {   // tabs in a column: full-width list, no width maths
    for (const el of tabEls) el.style.width = '';
    return;
  }
  const row = $('row1').clientWidth;
  const ctlInRow = $('winctl').parentNode === $('row1') ? $('winctl').offsetWidth + 14 : 0;
  // Group labels take room too; tabs of collapsed groups don't.
  const chipsW = [...chipById.values()].filter((c) => c.isConnected).reduce((a, c) => a + c.offsetWidth + 3, 0);
  const visible = tabEls.filter((el) => !el.classList.contains('gcollapsed')).length || 1;
  const used = 14 + $('newtab').offsetWidth + 6 + ctlInRow + 40 + chipsW;
  let w = Math.floor((row - used) / visible) - TAB_GAP;
  w = Math.max(TAB_MIN, Math.min(TAB_MAX, w));
  if (frozenW) w = Math.min(w, Math.round(frozenW));
  for (const el of tabEls) el.style.width = w + 'px';
}
$('tabs').addEventListener('mouseleave', () => { if (frozenW && !tabDrag) { frozenW = 0; layoutTabs(); } });
window.addEventListener('resize', layoutTabs);

// ---------- drag tabs: sort them, or drop outside the window for a new window ----------
let tabDrag = null;
function startTabDrag(e, el) {
  tabDrag = { el, from: el._i, x0: e.clientX, y0: e.clientY, on: false, to: el._i, out: false };
  el.setPointerCapture(e.pointerId);
  el.addEventListener('pointermove', onTabDragMove);
  el.addEventListener('pointerup', onTabDragEnd);
  el.addEventListener('pointercancel', onTabDragEnd);
}
function onTabDragMove(e) {
  const d = tabDrag;
  if (!d) return;
  const ax = AX('tabs');
  const dx = ax.pos === 'clientY' ? e.clientY - d.y0 : e.clientX - d.x0;
  if (!d.on) {
    if (Math.abs(e.clientX - d.x0) < 6 && Math.abs(e.clientY - d.y0) < 6) return;
    d.on = true;
    d.el.classList.add('dragging');
    d.w = d.el.getBoundingClientRect()[ax.size] + (ax.size === 'height' ? 4 : TAB_GAP);
    d.rects = tabEls.map((x) => x.getBoundingClientRect());   // positions before anything moves
    d.overK = -1;
    d.ready = false;
  }
  // Outside the window: the tab will open in a new window when dropped.
  const out = e.clientY < -30 || e.clientY > window.innerHeight + 40 || e.clientX < -30 || e.clientX > window.innerWidth + 30;
  if (out !== d.out) {
    d.out = out;
    d.el.classList.toggle('detaching', out);
    document.body.classList.toggle('detaching', out);
  }
  // The tab follows the mouse (kept inside the tab row).
  const hidden = (k) => tabEls[k].classList.contains('gcollapsed');
  const vis = d.rects.map((r, k) => k).filter((k) => !hidden(k));
  const first = d.rects[vis[0]], last = d.rects[vis[vis.length - 1]], mine = d.rects[d.from];
  const tx = Math.max(first[ax.start] - mine[ax.start], Math.min(last[ax.end] - mine[ax.end], dx));
  d.el.style.transform = ax.tr + '(' + tx + 'px)';
  // Where it would go, and which tab's middle it is over (to group with it).
  const p = e[ax.pos];
  let lastLeft = -1, over = -1;
  for (const k of vis) {
    if (k === d.from) continue;
    const r = d.rects[k];
    if (p > r[ax.start] + r[ax.size] / 2) lastLeft = k;
    const rel = (p - r[ax.start]) / r[ax.size];
    if (rel > 0.3 && rel < 0.7) over = k;
  }
  const to = lastLeft === -1 ? 0 : (lastLeft < d.from ? lastLeft + 1 : lastLeft);
  d.to = to;
  // Hold over the middle of a tab (~0.35 s) -> they will be grouped on release.
  if (over !== d.overK) {
    d.overK = over;
    d.ready = false;
    clearTimeout(d.holdT);
    tabEls.forEach((t) => t.classList.remove('group-target'));
    if (over >= 0) {
      d.holdT = setTimeout(() => {
        if (tabDrag !== d || d.overK !== over) return;
        d.ready = true;
        const target = tabEls[over];
        target.classList.add('group-target');
        target.style.setProperty('--gt', target.style.getPropertyValue('--g') || getComputedStyle(document.body).getPropertyValue('--accent'));
        tabEls.forEach((x) => { if (x !== d.el) x.style.transform = ''; });   // nothing slides while grouping
      }, 350);
    }
  }
  if (d.ready) return;
  // The others slide aside to make room.
  tabEls.forEach((t, k) => {
    if (t === d.el) return;
    let shift = 0;
    if (k > d.from && k <= to) shift = -d.w;
    else if (k < d.from && k >= to) shift = d.w;
    t.style.transform = shift ? ax.tr + '(' + shift + 'px)' : '';
  });
}
function onTabDragEnd(e) {
  const d = tabDrag;
  if (!d) return;
  d.el.removeEventListener('pointermove', onTabDragMove);
  d.el.removeEventListener('pointerup', onTabDragEnd);
  d.el.removeEventListener('pointercancel', onTabDragEnd);
  tabDrag = null;
  document.body.classList.remove('detaching');
  if (!d.on) return;
  d.el.classList.remove('detaching');
  clearTimeout(d.holdT);
  tabEls.forEach((t) => t.classList.remove('group-target'));
  // Held over another tab's middle: group them.
  if (d.ready && d.overK >= 0 && e.type === 'pointerup') {
    tabEls.forEach((t) => { t.style.transform = ''; });
    d.el.classList.remove('dragging');
    post({ cmd: 'tab_group', from: d.from, target: d.overK });
    return;
  }
  if (d.out && e.type === 'pointerup' && tabEls.length > 1) {
    tabEls.forEach((t) => { t.style.transform = ''; });
    d.el.classList.remove('dragging');
    post({ cmd: 'tab_detach', index: d.from });
    return;
  }
  // Settle: move the element in the row now (no jump), tell the browser.
  const moved = d.to !== d.from && e.type === 'pointerup';
  const ax = AX('tabs');
  const rectBefore = d.el.getBoundingClientRect()[ax.start];
  tabEls.forEach((t) => { t.style.transition = 'none'; t.style.transform = ''; });
  if (moved) {
    const [el] = tabEls.splice(d.from, 1);
    tabEls.splice(d.to, 0, el);
    const t = state.tabs.splice(d.from, 1)[0];
    state.tabs.splice(d.to, 0, t);
    const box = $('tabs');
    tabEls.forEach((x) => box.insertBefore(x, $('newtab')));
    tabEls.forEach((x, k) => { x._i = k; });
    post({ cmd: 'move_tab', from: d.from, to: d.to });
  }
  // Glide the dropped tab from where it was let go into its slot.
  const offset = rectBefore - d.el.getBoundingClientRect()[ax.start];
  d.el.style.transform = ax.tr + '(' + offset + 'px)';
  void d.el.offsetWidth;
  tabEls.forEach((t) => { t.style.transition = ''; });
  d.el.style.transform = '';
  setTimeout(() => d.el.classList.remove('dragging'), ANIM_MS);
}

let addrTab = -1;   // the tab whose address the field shows
function renderState() {
  const addr = $('addr');
  // While typing, the field is left alone; another tab always shows its own address.
  if (document.activeElement !== addr || addrTab !== state.active) addr.value = state.url;
  addrTab = state.active;
  const star = $('star');
  star.classList.toggle('on', !!state.star);
  star.disabled = !state.url || !!state.inc;
  star.title = state.star ? 'Saved - click to remove from the saved tabs bar' : 'Save this website to the saved tabs bar';
  $('back').disabled = !state.back;
  $('fwd').disabled = !state.fwd;
  if (document.body.classList.contains('max') !== !!state.max) {
    document.body.classList.toggle('max', !!state.max);
    applyRows();   // the resize strip above the page comes and goes
  }
  document.body.classList.toggle('inc', !!state.inc);
  const maxIcon = state.max ? 'restore' : 'max';
  if ($('max').dataset.icon !== maxIcon) { $('max').dataset.icon = maxIcon; $('max').innerHTML = WIN_ICONS[maxIcon]; }
  $('max').title = state.max ? 'Restore' : 'Maximize';
}

// Window button icons (drawn, so they look the same on every system).
const WIN_SVG = (d) => '<svg viewBox="0 0 10 10" width="10" height="10" fill="none" stroke="currentColor" stroke-width="1" shape-rendering="crispEdges">' + d + '</svg>';
const WIN_ICONS = {
  min: WIN_SVG('<path d="M0 5.5h10"/>'),
  max: WIN_SVG('<rect x=".5" y=".5" width="9" height="9"/>'),
  restore: WIN_SVG('<path d="M2.5 2.5V.5h7v7h-2"/><rect x=".5" y="2.5" width="7" height="7"/>'),
  close: WIN_SVG('<path d="M.5.5l9 9M9.5.5l-9 9" shape-rendering="geometricPrecision"/>'),
};
$('min').innerHTML = WIN_ICONS.min;
$('close').innerHTML = WIN_ICONS.close;
$('max').innerHTML = WIN_ICONS.max;
$('max').dataset.icon = 'max';

// ---------- saved tabs (row 3) ----------
// Saved-tab icons follow the icon style (class "ico"; "f" = filled in Filled/Duotone).
const FOLDER_ICON = '<svg class="ico" viewBox="0 0 16 16"><path class="f" d="M1.8 4A1.3 1.3 0 0 1 3.1 2.7h3l1.5 1.6h5.3a1.3 1.3 0 0 1 1.3 1.3V12a1.3 1.3 0 0 1-1.3 1.3H3.1A1.3 1.3 0 0 1 1.8 12z"/></svg>';
const GLOBE_ICON = '<svg class="ico" viewBox="0 0 16 16" style="stroke-width:calc(var(--ic-w) * .8)"><circle class="d" cx="8" cy="8" r="6.3"/><path d="M1.7 8h12.6M8 1.7c2 2 2 10.6 0 12.6M8 1.7c-2 2-2 10.6 0 12.6"/></svg>';
let saved = [];
const isFolder = (n) => n && n.f !== undefined;

function renderSaved() {
  const box = $('saved');
  box.textContent = '';
  if (!saved.length) {
    const h = document.createElement('span');
    h.className = 'saved-hint';
    h.textContent = 'Right-click a tab and choose "Save" to keep it here';
    box.appendChild(h);
    return;
  }
  saved.forEach((n, i) => {
    const el = document.createElement('div');
    el.className = 'saved-item' + (isFolder(n) ? ' folder' : '');
    el.dataset.index = i;
    if (isFolder(n)) {
      el.insertAdjacentHTML('beforeend', FOLDER_ICON);
      el.title = n.f + ' (' + n.n + ')';
    } else if (n.i) {
      const img = document.createElement('img');
      img.src = n.i;
      img.onerror = () => { img.replaceWith(document.createRange().createContextualFragment(GLOBE_ICON)); };
      toneIcon(img, n.i);
      el.appendChild(img);
      el.title = n.t + '\n' + n.u;
    } else {
      el.insertAdjacentHTML('beforeend', GLOBE_ICON);
      el.title = n.t + '\n' + n.u;
    }
    const t = document.createElement('span');
    t.className = 'title';
    t.textContent = isFolder(n) ? n.f : n.t;
    el.appendChild(t);
    box.appendChild(el);
  });
}

function savedEls() { return [...$('saved').querySelectorAll('.saved-item')]; }

// Drag: press and move. Drop between items = move there; on the middle of a
// link = new folder with both; on the middle of a folder = put it inside.
// While dragging, the item follows the mouse and the others slide aside to
// open a gap where it will land (positions measured once, before anything moves).
let drag = null;
function clearMarks() {
  savedEls().forEach((e) => { e.classList.remove('drop-into'); e.style.transform = ''; });
}
function dropTarget(x) {
  const rects = drag.rects;
  const from = drag.i;
  const dragged = saved[from];
  const ax = AX('saved');
  for (let k = 0; k < rects.length; k++) {
    const r = rects[k];
    if (x > r[ax.end] + 1 && k < rects.length - 1) continue;
    const rel = (x - r[ax.start]) / r[ax.size];
    if (k !== from && !isFolder(dragged) && rel > 0.28 && rel < 0.72) {
      return { kind: isFolder(saved[k]) ? 'into' : 'merge', target: k };
    }
    const before = rel < 0.5 ? k : k + 1;
    if (before === from || before === from + 1) return null;   // same place
    return { kind: 'before', target: before };
  }
  return null;
}
function showDrop(x) {
  const els = savedEls();
  const d = drag;
  d.drop = dropTarget(x);
  els.forEach((el, k) => {
    if (k === d.i) return;
    let shift = 0;
    if (d.drop && d.drop.kind === 'before') {
      const t = d.drop.target;
      if (k > d.i && k < t) shift = -d.w;          // moving right: items in between slide left
      else if (k < d.i && k >= t) shift = d.w;     // moving left: items in between slide right
    }
    el.style.transform = shift ? AX('saved').tr + '(' + shift + 'px)' : '';
    el.classList.toggle('drop-into', !!(d.drop && d.drop.kind !== 'before' && d.drop.target === k));
  });
}
$('saved').addEventListener('pointerdown', (e) => {
  const el = e.target.closest('.saved-item');
  if (!el || e.button !== 0 || el.querySelector('input')) return;
  drag = { el, i: +el.dataset.index, x: e.clientX, y: e.clientY, on: false, drop: null, ctrl: e.ctrlKey, shift: e.shiftKey };
  el.setPointerCapture(e.pointerId);
});
$('saved').addEventListener('pointermove', (e) => {
  if (!drag) return;
  if (!drag.on && Math.hypot(e.clientX - drag.x, e.clientY - drag.y) < 5) return;
  if (!drag.on) {
    drag.on = true;
    drag.rects = savedEls().map((x) => x.getBoundingClientRect());
    drag.w = drag.rects[drag.i][AX('saved').size] + 2;     // item size + gap
    drag.el.classList.add('dragging');
  }
  const ax = AX('saved');
  const along = ax.pos === 'clientY' ? e.clientY - drag.y : e.clientX - drag.x;
  drag.el.style.transform = ax.tr + '(' + along + 'px)';
  showDrop(e[ax.pos]);
});
function endDrag(e, cancelled) {
  if (!drag) return;
  const d = drag;
  drag = null;
  if (cancelled || !d.on) {
    clearMarks();
    d.el.classList.remove('dragging');
    d.el.style.transform = '';
    if (cancelled) return;
  }
  if (d.on) {
    const drop = d.drop;
    if (drop && drop.kind === 'before') {
      // Reorder right away (the browser confirms with the same order).
      const els = savedEls();
      const ax = AX('saved');
      const before = d.el.getBoundingClientRect()[ax.start];
      els.forEach((x) => { x.style.transition = 'none'; x.style.transform = ''; });
      const to = drop.target > d.i ? drop.target - 1 : drop.target;
      const [item] = saved.splice(d.i, 1);
      saved.splice(to, 0, item);
      renderSaved();
      // Glide the dropped item from where it was let go into its place.
      const el = savedEls()[to];
      if (el) {
        el.style.transition = 'none';
        el.style.transform = ax.tr + '(' + (before - el.getBoundingClientRect()[ax.start]) + 'px)';
        void el.offsetWidth;
        el.style.transition = '';
        el.style.transform = '';
      }
    } else {
      clearMarks();
      d.el.classList.remove('dragging');
      d.el.style.transform = '';
    }
    if (drop) post({ cmd: 'saved_move', path: [d.i], kind: drop.kind, target: drop.target });
    return;
  }
  const n = saved[d.i];
  if (isFolder(n)) {
    const r = d.el.getBoundingClientRect();
    post({ cmd: 'saved_folder', path: [d.i], x: r.left, y: r.bottom + 4 });
  } else {
    post({ cmd: 'saved_open', path: [d.i], how: d.ctrl ? 'bg' : d.shift ? 'new' : 'same' });
  }
}
$('saved').addEventListener('pointerup', (e) => endDrag(e, false));
$('saved').addEventListener('pointercancel', (e) => endDrag(e, true));
$('saved').addEventListener('auxclick', (e) => {
  const el = e.target.closest('.saved-item');
  if (el && e.button === 1 && !isFolder(saved[+el.dataset.index])) post({ cmd: 'saved_open', path: [+el.dataset.index], how: 'bg' });
});
$('row3').addEventListener('contextmenu', (e) => {
  e.preventDefault();
  const el = e.target.closest('.saved-item');
  post({ cmd: 'saved_menu', path: el ? [+el.dataset.index] : null, x: e.clientX, y: e.clientY });
});

function startRename(path) {
  const el = savedEls()[path[0]];
  if (!el) return;
  const t = el.querySelector('.title');
  const inp = document.createElement('input');
  inp.className = 'rename';
  inp.value = t.textContent;
  inp.spellcheck = false;
  t.replaceWith(inp);
  inp.focus();
  inp.select();
  let done = false;
  const finish = (ok) => {
    if (done) return;
    done = true;
    const v = inp.value.trim();
    // Shown right away: the browser only answers when the list really changed.
    const n = saved[path[0]];
    if (ok && v && n) {
      if (isFolder(n)) n.f = v; else n.t = v;
      post({ cmd: 'saved_rename', path, title: v });
    }
    renderSaved();
  };
  inp.addEventListener('keydown', (e) => {
    if (e.key === 'Enter') finish(true);
    else if (e.key === 'Escape') finish(false);
    e.stopPropagation();
  });
  const started = Date.now();
  inp.addEventListener('blur', () => {
    // Focus may flicker right after a menu closes; keep editing then.
    if (Date.now() - started < 600) { setTimeout(() => inp.focus(), 50); return; }
    finish(true);
  });
  inp.addEventListener('pointerdown', (e) => e.stopPropagation());
}

// ---------- resizable rows (drag the line under a row; ≡ -> Lock bar sizes) ----------
const ROW_LIMITS = [[28, 72], [34, 80], [22, 56]];   // must match settings.rs
let rows = [40, 46, 30];
// Row order (Settings -> Layout): rows at the same edge are stacked in this order.
let order = ['tabs', 'nav', 'saved'];
// Where each row is: top, bottom, left or right (Settings -> Layout).
let pos = { tabs: 'top', nav: 'top', saved: 'top' };
const ROW_EL = { tabs: 'row1', nav: 'row2', saved: 'row3' };
const SIZER = { tabs: 'sz-tabs', nav: 'sz-nav', saved: 'sz-saved' };
const sizerDir = { tabs: 1, nav: 1, saved: 1 };   // +1: drag down = taller, -1: drag up = taller
// The bar fills the whole window; rows sit at the edges and the page covers the
// rest (#hole). Rust computes the same rectangle for the page (ui.rs edges()).
function applyRows() {
  const st = document.documentElement.style;
  st.setProperty('--row1', rows[0] + 'px');
  st.setProperty('--row2', rows[1] + 'px');
  st.setProperty('--row3', rows[2] + 'px');
  st.setProperty('--side-w', sideW + 'px');
  const h = { tabs: rows[0], nav: rows[1], saved: rows[2] };
  const showSaved = document.body.classList.contains('saved');
  const vis = order.filter((k) => k !== 'saved' || showSaved);
  const at = (p) => vis.filter((k) => pos[k] === p);
  const top = at('top'), bot = at('bottom'), left = at('left'), right = at('right');
  const topH = top.reduce((a, k) => a + h[k], 0);
  const botH = bot.reduce((a, k) => a + h[k], 0);
  order.forEach((k, i) => st.setProperty('--o-' + k, i));      // order inside a column
  // Rows at the top / bottom: full width, stacked.
  let y = 0;
  for (const k of top) {
    const el = $(ROW_EL[k]);
    if (el.parentNode !== document.body) document.body.insertBefore(el, $('sz-tabs'));
    el.style.top = y + 'px';
    el.style.bottom = '';
    $(SIZER[k]).style.top = (y + h[k] - 3) + 'px';
    sizerDir[k] = 1;
    y += h[k];
  }
  let below = botH;
  for (const k of bot) {
    const el = $(ROW_EL[k]);
    if (el.parentNode !== document.body) document.body.insertBefore(el, $('sz-tabs'));
    el.style.top = 'calc(100% - ' + below + 'px)';
    el.style.bottom = '';
    $(SIZER[k]).style.top = 'calc(100% - ' + (below + 2) + 'px)';
    sizerDir[k] = -1;
    below -= h[k];
  }
  // Rows at the left / right: into that column.
  for (const [col, list] of [['colL', left], ['colR', right]]) {
    const c = $(col);
    c.classList.toggle('on', list.length > 0);
    c.style.top = topH + 'px';
    c.style.bottom = botH + 'px';
    for (const k of list) {
      const el = $(ROW_EL[k]);
      if (el.parentNode !== c) c.appendChild(el);
      el.style.top = '';
      el.style.bottom = '';
    }
  }
  for (const k of order) $(SIZER[k]).classList.toggle('off', !vis.includes(k) || pos[k] === 'left' || pos[k] === 'right');
  // The page: everything else (a thin strip stays at the top for resizing).
  const hole = $('hole');
  const strip = topH ? 0 : (document.body.classList.contains('max') ? 0 : 4);
  hole.style.left = (left.length ? sideW : 0) + 'px';
  hole.style.right = (right.length ? sideW : 0) + 'px';
  hole.style.top = (topH || strip) + 'px';
  hole.style.bottom = botH + 'px';
  // Tabs point at the address bar when it is at the same edge, else at the page.
  const same = pos.tabs === pos.nav && (pos.tabs === 'top' || pos.tabs === 'bottom');
  document.body.classList.toggle('tabs-below', same ? order.indexOf('tabs') > order.indexOf('nav') : pos.tabs === 'bottom');
  placeWindowButtons(top);
  layoutTabs();
}

// Window buttons: right end of the top-most row. No row at the top: Suppa
// draws them itself, floating over the page (winbtn.rs).
let nativeButtons = true;   // false on Linux: the buttons always stay in the bar
function placeWindowButtons(top) {
  const ctl = $('winctl');
  const vis = order.filter((k) => k !== 'saved' || document.body.classList.contains('saved'));
  const first = top[0] || (nativeButtons ? '' : vis[0]);
  ctl.classList.toggle('away', !first);
  if (first) {
    const row = $(ROW_EL[first]);
    if (ctl.parentNode !== row) row.appendChild(ctl);
  }
  document.body.classList.toggle('tabs-not-top', first !== 'tabs');
}
let rowsTimer = 0;
function sendRows(save) {
  if (!save && rowsTimer) return;
  const send = () => { rowsTimer = 0; post({ cmd: 'rows', tabs: rows[0], nav: rows[1], saved: rows[2], side: sideW, save }); };
  if (save) { clearTimeout(rowsTimer); send(); } else rowsTimer = setTimeout(send, 30);
}
[['sz-tabs', 0], ['sz-nav', 1], ['sz-saved', 2]].forEach(([id, k]) => {
  const el = $(id);
  el.addEventListener('pointerdown', (e) => {
    if (e.button !== 0 || document.body.classList.contains('locked')) return;
    e.preventDefault();
    el.setPointerCapture(e.pointerId);
    el.classList.add('on');
    const y0 = e.screenY, start = rows[k];
    const move = (ev) => {
      const [lo, hi] = ROW_LIMITS[k];
      const dir = sizerDir[['tabs', 'nav', 'saved'][k]];
      const v = Math.max(lo, Math.min(hi, Math.round(start + dir * (ev.screenY - y0))));
      if (v !== rows[k]) { rows[k] = v; applyRows(); sendRows(false); }
    };
    const up = () => {
      el.classList.remove('on');
      el.removeEventListener('pointermove', move);
      el.removeEventListener('pointerup', up);
      el.removeEventListener('pointercancel', up);
      sendRows(true);
    };
    el.addEventListener('pointermove', move);
    el.addEventListener('pointerup', up);
    el.addEventListener('pointercancel', up);
  });
  // Double-click a line: back to the default size.
  el.addEventListener('dblclick', () => {
    if (document.body.classList.contains('locked')) return;
    rows[k] = [40, 46, 30][k];
    applyRows();
    sendRows(true);
  });
});

// Column width: drag its inner edge (left column: right edge, right column: left edge).
let sideW = 260;
document.querySelectorAll('.sz-col').forEach((el) => {
  el.addEventListener('pointerdown', (e) => {
    if (e.button !== 0 || document.body.classList.contains('locked')) return;
    e.preventDefault();
    el.setPointerCapture(e.pointerId);
    el.classList.add('on');
    const x0 = e.screenX, start = sideW, dir = el.parentNode.id === 'colR' ? -1 : 1;
    const move = (ev) => {
      const v = Math.max(180, Math.min(480, Math.round(start + dir * (ev.screenX - x0) / (window.devicePixelRatio || 1))));
      if (v !== sideW) { sideW = v; applyRows(); sendRows(false); }
    };
    const up = () => {
      el.classList.remove('on');
      el.removeEventListener('pointermove', move);
      el.removeEventListener('pointerup', up);
      el.removeEventListener('pointercancel', up);
      sendRows(true);
    };
    el.addEventListener('pointermove', move);
    el.addEventListener('pointerup', up);
    el.addEventListener('pointercancel', up);
  });
  el.addEventListener('dblclick', () => { if (!document.body.classList.contains('locked')) { sideW = 260; applyRows(); sendRows(true); } });
});

// ---------- extension buttons (right of the address bar) ----------
function renderExts(items) {
  const box = $('exts');
  box.textContent = '';
  for (const x of items) {
    const b = document.createElement('button');
    b.title = x.name;
    if (x.icon) {
      const img = document.createElement('img');
      img.src = x.icon;
      b.appendChild(img);
    } else {
      const l = document.createElement('span');
      l.className = 'letter';
      l.textContent = (x.name || '?').trim().charAt(0).toUpperCase();
      b.appendChild(l);
    }
    const at = () => { const r = b.getBoundingClientRect(); return { x: r.right, y: r.bottom + 4 }; };
    b.addEventListener('click', () => post({ cmd: 'ext_click', id: x.id, ...at() }));
    b.addEventListener('contextmenu', (e) => { e.preventDefault(); post({ cmd: 'ext_menu', id: x.id, x: e.clientX, y: e.clientY }); });
    box.appendChild(b);
  }
}

// ---------- messages from the browser ----------
window.chrome.webview.addEventListener('message', (e) => {
  const m = e.data;
  if (m.type === 'init') {
    accent = m.accent || 'cycle';
    onAccent = m.on_accent || 'auto';
    applyColors(m.colors);
    document.body.classList.toggle('saved', !!m.saved);
    document.body.classList.toggle('locked', !!m.locked);
    if (Array.isArray(m.order) && m.order.length === 3) order = m.order;
    if (m.pos) pos = { tabs: m.pos.tabs || 'top', nav: m.pos.nav || 'top', saved: m.pos.saved || 'top' };
    else if (m.bar_pos) pos = { tabs: m.bar_pos, nav: m.bar_pos, saved: m.bar_pos };
    nativeButtons = m.native_buttons !== false;
    if (typeof m.side_width === 'number') sideW = m.side_width;
    // Icon style, corner style, tab corners
    for (const p of ['bold', 'sharp', 'filled', 'duotone']) document.body.classList.toggle('ic-' + p, m.icons === p);
    document.body.classList.toggle('squircle', !!m.squircle);
    if (typeof m.radius === 'number') {
      // Squircle corners look smaller than round ones of the same size, so they get a bigger radius.
      const r = m.squircle ? Math.round(m.radius * 1.6) : m.radius;
      document.documentElement.style.setProperty('--radius', r + 'px');
    }
    const tc = /^[01]{4}$/.test(m.tab_corners || '') ? m.tab_corners : '';
    document.body.classList.toggle('tab-corners', !!tc);
    if (tc) document.body.style.setProperty('--tab-r', [...tc].map((c) => c === '1' ? 'var(--radius)' : '0').join(' '));
    if (!m.rows) applyRows();
    if (m.rows) { rows = m.rows.slice(); applyRows(); }
    setTheme(m.dark, true);
  } else if (m.type === 'sync') {
    state = m;
    renderTabs();
    renderState();
  } else if (m.type === 'saved') {
    const items = m.items || [];
    if (JSON.stringify(items) === JSON.stringify(saved) && $('saved').childElementCount) return;  // already shown
    saved = items;
    renderSaved();
  } else if (m.type === 'external_drop') {
    // A link dragged out of a folder menu and let go over the bar.
    const r3 = $('row3').getBoundingClientRect();
    if (m.y < r3.top - 4 || m.y > r3.bottom + 4 || m.x < r3.left - 4 || m.x > r3.right + 4 || !document.body.classList.contains('saved')) return;
    const els = savedEls();
    const ax = AX('saved');
    const p = ax.pos === 'clientY' ? m.y : m.x;
    let drop = { kind: 'before', target: els.length };
    for (let k = 0; k < els.length; k++) {
      const r = els[k].getBoundingClientRect();
      if (p > r[ax.end] + 1 && k < els.length - 1) continue;
      const rel = (p - r[ax.start]) / r[ax.size];
      if (rel > 0.28 && rel < 0.72) drop = { kind: isFolder(saved[k]) ? 'into' : 'merge', target: k };
      else drop = { kind: 'before', target: rel < 0.5 ? k : k + 1 };
      break;
    }
    post({ cmd: 'saved_move', path: m.path, kind: drop.kind, target: drop.target });
  } else if (m.type === 'exts') {
    renderExts(m.items || []);
    layoutTabs();
  } else if (m.type === 'group_rename') {
    startGroupRename(m.id);
  } else if (m.type === 'rename') {
    startRename(m.path);
  } else if (m.type === 'dl') {
    // Download button next to the address bar (≡ -> Show download button);
    // the accent fills it clockwise with the progress of running downloads.
    const b = $('dl');
    const wasHidden = b.hidden;
    b.hidden = !m.show;
    clearTimeout(b._done);
    if (m.active > 0) {
      if (!b.classList.contains('busy')) {
        // A new round: start from empty, without animating back.
        b.classList.add('reset');
        b.style.setProperty('--p', 0);
        void b.offsetWidth;
        b.classList.remove('reset');
        b.classList.add('busy');
      }
      // Size unknown: a small wedge so it still shows something is running.
      b.style.setProperty('--p', m.p >= 0 ? Math.min(1, m.p) : 0.12);
    } else if (b.classList.contains('busy')) {
      // Finished: fill up, then fade back to a normal button.
      b.style.setProperty('--p', 1);
      b._done = setTimeout(() => b.classList.remove('busy'), 900);
    }
    b.title = m.active > 0 ? 'Downloads - ' + m.active + ' running' : 'Downloads';
    if (wasHidden !== b.hidden) layoutTabs();
  } else if (m.type === 'dl_open') {
    // A download started: its popup opens under this button (or ≡).
    const b = $('dl').hidden ? $('menu') : $('dl');
    const r = b.getBoundingClientRect();
    post({ cmd: 'menu', which: 'downloads', x: r.right, y: r.bottom + 4 });
  } else if (m.type === 'update') {
    const b = $('menu');
    b.classList.toggle('hot', !!m.available && !m.building);
    b.classList.toggle('busy', !!m.building);
    b.querySelector('.upd').textContent = m.building ? 'Building…' : 'Update';
    b.title = m.building
      ? 'Building the new version of Suppa...'
      : m.available
        ? 'Changes are ready (scripts folder or Rust code) - open the menu and click Update'
        : 'Menu: Update, Extensions, Ad blocker, Log, Theme, Settings';
    layoutTabs();
  } else if (m.type === 'focus_address') {
    const a = $('addr');
    a.focus();
    a.select();
  }
});

// ---------- buttons ----------
const run = (name) => post({ cmd: 'run', name });
$('newtab').onclick = () => run('new_tab');
$('back').onclick = () => run('back');
$('fwd').onclick = () => run('forward');
$('reload').onclick = () => run('reload');
$('home').onclick = () => run('home');
$('star').onclick = () => post({ cmd: 'star' });
$('min').onclick = () => run('minimize');
$('max').onclick = () => run('maximize');
$('close').onclick = () => run('close_window');
$('dl').onclick = () => {
  const r = $('dl').getBoundingClientRect();
  post({ cmd: 'menu', which: 'downloads', x: r.right, y: r.bottom + 4 });
};
$('menu').onclick = () => {
  const r = $('menu').getBoundingClientRect();
  post({ cmd: 'menu', which: 'main', x: r.right, y: r.bottom + 4 });
};

// ---------- window dragging (the tab row is the title bar) ----------
let lastDown = 0;
// Empty space drags the window: in the tab row, and in the top row when the
// tabs are lower down (then that row holds the window buttons).
function isCaption(el) {
  // Empty space of any row or column (rows can be at any edge).
  return [$('row1'), $('tabs'), $('drag'), $('row2'), $('row3'), $('saved'), document.body].includes(el) || el.classList.contains('col');
}
const onCaptionDown = (e) => {
  if (e.button !== 0 || !isCaption(e.target)) return;
  const now = Date.now();
  if (now - lastDown < 400) {           // double-click: maximize / restore
    lastDown = 0;
    run('maximize');
    return;
  }
  lastDown = now;
  post({ cmd: 'drag', edge: 'caption' });
};
for (const id of ['row1', 'row2', 'row3', 'colL', 'colR']) $(id).addEventListener('mousedown', onCaptionDown);
for (const [id, edge] of [['grip-top', 'top'], ['grip-tl', 'topleft'], ['grip-tr', 'topright']]) {
  $(id).addEventListener('mousedown', (e) => { if (e.button === 0) post({ cmd: 'drag', edge }); });
}

// ---------- address bar ----------
const addr = $('addr');
let selectOnClick = false;
addr.addEventListener('focus', () => { selectOnClick = true; });
addr.addEventListener('mouseup', () => {
  if (selectOnClick && addr.selectionStart === addr.selectionEnd) addr.select();
  selectOnClick = false;
});
addr.addEventListener('keydown', (e) => {
  if (e.key === 'Enter') {
    let text = addr.value.trim();
    if (e.ctrlKey && text && !/[.\s]/.test(text)) text = 'https://www.' + text + '.com';
    if (text) post({ cmd: 'navigate', text });
    addr.blur();
  } else if (e.key === 'Escape') {
    addr.value = state.url;
    addr.blur();
    post({ cmd: 'focus_page' });
  }
});
addr.addEventListener('blur', () => { addr.value = state.url; });

// No browser context menu / text drag on the bar itself.
// A rename box (saved tab, folder, tab group) ends when you click anywhere else
// in the bar, or anywhere outside it (the page, another window).
const blurRename = (e) => {
  const a = document.activeElement;
  if (a && a.matches && a.matches('input.rename') && (!e || e.target !== a)) a.blur();
};
document.addEventListener('pointerdown', blurRename, true);
window.addEventListener('blur', () => blurRename());
document.addEventListener('contextmenu', (e) => { if (e.target !== addr && !e.target.closest('input.rename')) e.preventDefault(); });

post({ cmd: 'ready' });
