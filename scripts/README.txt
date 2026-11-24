Suppa Browser - scripts folder
=============================

Everything in this folder is loaded when Suppa starts. Edit any file, then press
"Update" in the browser (it lights up when it sees a change). Suppa saves your
tabs, closes and reopens with the new files.

  ui.html / ui.css / ui.js   the top bar (tabs, buttons, address bar, colours)
  newtab.html                the new tab page ({{SEARCH}} = search engine URL)
  userscripts\*.js           your own scripts, run on every web page
                             - add "// @match https://*.example.com/*" lines to
                               limit a script to some sites
                             - files starting with "_" are ignored
  filters\*.txt              your own ad-block rules (EasyList / uBlock syntax)

If you delete this folder, Suppa recreates the default files next to Suppa.exe.
