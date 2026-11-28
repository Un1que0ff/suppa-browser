// Suppa Browser - media info and per-tab volume. Runs in every page and frame.
//  * Tells Suppa what plays (media session title / artist / cover, or the
//    playing <video>/<audio>) for the player button next to the downloads.
//  * window.__suppaMedia(action): play / pause / next / previous from the player.
//  * window.__suppaVol(0..1): this tab's own volume (the page's volume times
//    this), for <video>, <audio> and Web Audio - independent of Windows' volume.
// Event driven (no polling), so idle pages cost nothing.
// Edit, then press "Update" in the browser.
(function () {
  if (window.__suppaMediaInit) return;
  window.__suppaMediaInit = true;
  const top = window.top === window;
  let factor = 1;
  let last = '';
  let timer = 0;

  // ---- per-tab volume -------------------------------------------------------
  // The page sees and sets its own volume as usual; what plays is that x factor.
  const desc = Object.getOwnPropertyDescriptor(HTMLMediaElement.prototype, 'volume');
  const wanted = new WeakMap();   // element -> volume the page asked for
  const media = new Set();        // elements we have seen (weakly: cleaned below)
  if (desc && desc.set) {
    Object.defineProperty(HTMLMediaElement.prototype, 'volume', {
      configurable: true, enumerable: desc.enumerable,
      get() { return wanted.has(this) ? wanted.get(this) : desc.get.call(this); },
      set(v) {
        wanted.set(this, v);
        media.add(this);
        desc.set.call(this, Math.max(0, Math.min(1, v * factor)));
      },
    });
  }
  const applyEl = (el) => {
    try {
      const w = wanted.has(el) ? wanted.get(el) : desc.get.call(el);
      if (!wanted.has(el)) wanted.set(el, w);
      desc.set.call(el, Math.max(0, Math.min(1, w * factor)));
    } catch (e) {}
  };
  // Web Audio: everything that goes to the speakers passes one gain per context.
  const gains = [];
  const AN = window.AudioNode && AudioNode.prototype;
  if (AN && AN.connect && window.AudioDestinationNode) {
    const connect = AN.connect;
    const gainOf = new WeakMap();
    AN.connect = function (dest, ...rest) {
      if (dest instanceof AudioDestinationNode) {
        let g = gainOf.get(dest);
        if (!g) {
          g = dest.context.createGain();
          g.gain.value = factor;
          connect.call(g, dest);
          gainOf.set(dest, g);
          gains.push(g);
        }
        if (this !== g) return connect.call(this, g, ...rest);
      }
      return connect.call(this, dest, ...rest);
    };
  }
  function setVol(f) {
    factor = Math.max(0, Math.min(1, +f || 0));
    document.querySelectorAll('video, audio').forEach((el) => { media.add(el); applyEl(el); });
    for (const el of media) if (!el.isConnected && !el.src) media.delete(el); else applyEl(el);
    for (const g of gains) { try { g.gain.value = factor; } catch (e) {} }
    // Frames (embedded players) follow.
    for (let i = 0; i < window.frames.length; i++) {
      try { window.frames[i].postMessage({ __suppaVol: factor }, '*'); } catch (e) {}
    }
  }
  window.__suppaVol = setVol;
  window.addEventListener('message', (e) => {
    if (e.source === window.parent && e.data && typeof e.data.__suppaVol === 'number') setVol(e.data.__suppaVol);
  });
  // New media elements start at the tab's volume.
  document.addEventListener('play', (e) => { if (e.target instanceof HTMLMediaElement) { media.add(e.target); if (factor !== 1) applyEl(e.target); } }, true);

  // ---- media session (what plays, and the page's own next / previous) --------
  const handlers = {};
  const MS = window.MediaSession && MediaSession.prototype;
  if (MS && MS.setActionHandler) {
    const set = MS.setActionHandler;
    MS.setActionHandler = function (action, fn) {
      handlers[action] = fn;
      try { return set.call(this, action, fn); } finally { report(); }
    };
    const md = Object.getOwnPropertyDescriptor(MS, 'metadata');
    if (md && md.set) {
      Object.defineProperty(MS, 'metadata', {
        configurable: true, enumerable: md.enumerable, get: md.get,
        set(v) { md.set.call(this, v); report(); },
      });
    }
  }
  if (!top) return;   // the rest only for the page itself (embedded players keep their volume above)

  // Browser keys the page did not use itself (like Chrome, a page's own
  // Ctrl+K / Ctrl+U in an editor wins): Ctrl+D save, Ctrl+E / Ctrl+K search,
  // Ctrl+M mute, Ctrl+U page source.
  const KEYS = { d: 'star', e: 'search', k: 'search', m: 'mute', u: 'source' };
  addEventListener('keydown', (e) => {
    if (!e.ctrlKey || e.shiftKey || e.altKey || e.metaKey || e.repeat) return;
    const name = KEYS[(e.key || '').toLowerCase()];
    if (!name) return;
    // Checked after the page's own handlers ran.
    setTimeout(() => {
      if (e.defaultPrevented) return;
      try { window.chrome.webview.postMessage('suppa-key:' + name); } catch (x) {}
    }, 0);
  });

  function mainMedia() {
    const all = [...document.querySelectorAll('video, audio')].concat([...media].filter((m) => !m.isConnected));
    // Muted background / preview videos (shop banners, hover previews) don't count.
    const real = all.filter((m) => !m.muted && m.volume > 0 && (m.duration > 3 || m.duration === Infinity));
    const playing = real.filter((m) => !m.paused && !m.ended && m.readyState > 1);
    const pick = playing[0] || real.filter((m) => m.currentTime > 0).sort((a, b) => (b.duration || 0) - (a.duration || 0))[0];
    return pick || null;
  }

  function report() {
    if (!top) return;
    clearTimeout(timer);
    timer = setTimeout(send, 150);
  }
  function send() {
    const ms = navigator.mediaSession;
    const meta = ms && ms.metadata;
    const el = mainMedia();
    const has = !!(meta && meta.title) || !!el;
    let art = '';
    if (meta && meta.artwork && meta.artwork.length) {
      const size = (a) => parseInt((a.sizes || '0').split('x')[0], 10) || 0;
      const best = [...meta.artwork].sort((a, b) => size(b) - size(a))[0];
      art = best && /^https?:/.test(best.src) ? best.src : '';
    }
    const playing = el ? !el.paused && !el.ended : ms && ms.playbackState === 'playing';
    const info = {
      has,
      title: meta && meta.title || '',
      artist: meta && meta.artist || '',
      album: meta && meta.album || '',
      art,
      playing: !!playing,
      video: !!(el && el.tagName === 'VIDEO' && el.videoWidth > 0),
      pos: el && isFinite(el.currentTime) ? el.currentTime : 0,
      dur: el && isFinite(el.duration) ? el.duration : 0,
      next: !!handlers.nexttrack || !!document.querySelector('.ytp-next-button'),
      prev: !!handlers.previoustrack,
    };
    const key = JSON.stringify(Object.assign({}, info, { pos: 0 }));
    if (key === last) return;
    last = key;
    info.at = Date.now();
    try { window.chrome.webview.postMessage('suppa-media:' + JSON.stringify(info)); } catch (e) {}
  }
  for (const ev of ['play', 'pause', 'ended', 'loadedmetadata', 'emptied', 'seeked', 'ratechange']) document.addEventListener(ev, report, true);

  function click(sel) {
    const b = document.querySelector(sel);
    if (b) { b.click(); return true; }
    return false;
  }
  window.__suppaMedia = function (action) {
    const el = mainMedia();
    const call = (a) => { if (handlers[a]) { try { handlers[a]({ action: a }); return true; } catch (e) {} } return false; };
    switch (action) {
      case 'toggle':
        if (el) { if (el.paused) el.play().catch(() => call('play')); else el.pause(); }
        else if (navigator.mediaSession && navigator.mediaSession.playbackState === 'playing') call('pause');
        else call('play');
        break;
      case 'next': call('nexttrack') || click('.ytp-next-button') || click('[data-testid="control-button-skip-forward"]'); break;
      case 'prev': call('previoustrack') || (el && (el.currentTime = 0)); break;
      case 'back10': if (el) el.currentTime = Math.max(0, el.currentTime - 10); else call('seekbackward'); break;
      case 'fwd10': if (el) el.currentTime = Math.min(el.duration || 1e9, el.currentTime + 10); else call('seekforward'); break;
      case 'frame': {
        // Small preview of the playing video (works for normal and YouTube videos,
        // not for protected ones like Netflix: those stay a cover / title).
        if (!el || el.tagName !== 'VIDEO' || !el.videoWidth) return '';
        try {
          const c = document.createElement('canvas');
          c.width = 240;
          c.height = Math.round(240 * el.videoHeight / el.videoWidth);
          c.getContext('2d').drawImage(el, 0, 0, c.width, c.height);
          return c.toDataURL('image/jpeg', 0.7);
        } catch (e) { return ''; }
      }
    }
    report();
    return '';
  };
})();
