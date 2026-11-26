// Suppa Browser - YouTube ad skipper. Runs on youtube.com when the ad blocker
// is on for it (a backstop for ads the filter lists miss):
//  * a video ad is muted, jumped to its end and its "Skip" button clicked;
//  * ad slots in the feed, search, next to videos and over the player are hidden.
// Edit, then press "Update" in the browser.
(function () {
  if (window.__suppaYT) return;
  window.__suppaYT = true;
  if (!/(^|\.)youtube\.com$/.test(location.hostname) || window.top !== window) return;

  // Ad slots (hidden with CSS, so YouTube's own layout stays intact).
  const HIDE = [
    '#masthead-ad', '#player-ads', '#panels > ytd-engagement-panel-section-list-renderer[target-id="engagement-panel-ads"]',
    'ytd-ad-slot-renderer', 'ytd-in-feed-ad-layout-renderer', 'ytd-banner-promo-renderer',
    'ytd-promoted-sparkles-web-renderer', 'ytd-promoted-video-renderer', 'ytd-display-ad-renderer',
    'ytd-companion-slot-renderer', 'ytd-action-companion-ad-renderer', 'ytd-player-legacy-desktop-watch-ads-renderer',
    'ytd-statement-banner-renderer', 'ytd-search-pyv-renderer', 'ytm-promoted-sparkles-web-renderer',
    'ytd-rich-item-renderer:has(> #content > ytd-ad-slot-renderer)',
    'ytd-rich-section-renderer:has(ytd-statement-banner-renderer)',
    '.ytp-ad-overlay-container', '.ytp-ad-overlay-slot', '.ytp-featured-product', '.ytd-merch-shelf-renderer',
  ];
  const addCss = () => {
    const root = document.head || document.documentElement;
    if (!root) return false;
    const s = document.createElement('style');
    s.id = 'suppa-yt';
    s.textContent = HIDE.map((x) => x + '{display:none!important}').join('\n');
    root.appendChild(s);
    return true;
  };
  if (!addCss()) document.addEventListener('readystatechange', addCss, { once: true });

  // Video ads.
  const SKIP = '.ytp-skip-ad-button, .ytp-ad-skip-button, .ytp-ad-skip-button-modern, .ytp-ad-skip-button-slot button, button[id^="skip-button"]';
  let saved = null;   // the viewer's own sound and speed, put back after the ad
  let watched = null; // the player being watched for ads
  function check() {
    const player = document.querySelector('#movie_player, .html5-video-player');
    if (!player) return;
    if (player !== watched) {
      // Only the player's class changes matter (YouTube marks ads with "ad-showing").
      watched = player;
      new MutationObserver(check).observe(player, { attributes: true, attributeFilter: ['class'] });
    }
    const video = player.querySelector('video');
    const ad = player.classList.contains('ad-showing') || player.classList.contains('ad-interrupting');
    if (ad && video) {
      if (saved === null) saved = { muted: video.muted, rate: video.playbackRate > 4 ? 1 : video.playbackRate };
      video.muted = true;
      if (isFinite(video.duration) && video.duration > 0 && video.currentTime < video.duration - 0.1) {
        try { video.currentTime = video.duration; } catch (e) {}
      }
      try { video.playbackRate = 16; } catch (e) {}
      if (video.paused) video.play().catch(() => {});
      document.querySelectorAll(SKIP).forEach((b) => { try { b.click(); } catch (e) {} });
    } else if (!ad && saved !== null) {
      if (video) {
        video.muted = saved.muted;
        video.playbackRate = saved.rate;
      }
      saved = null;
    }
  }
  // The player appears later and is replaced on some page changes: look every 0.5 s.
  setInterval(check, 500);
  check();
})();
