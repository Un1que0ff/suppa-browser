// Example user script. Files starting with "_" are ignored - rename to "example.js" to enable.
// @match https://*.wikipedia.org/*
//
// Runs at document start on every matching page (before the page's own scripts).

document.addEventListener('DOMContentLoaded', () => {
  document.body.style.outline = '3px solid #b07cff';
});
