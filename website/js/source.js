/* Shows either "the Flatpak source is not active yet" or the update-source text, never a promise the site cannot keep.
   "auto": the site asks itself (relative addresses, so it works under /TSW-Forever/ and on localhost) whether the files of the signed source are really there:
   repo/summary, tsw-forever.flatpakrepo and the .flatpakref. All must answer with success. The host name is deliberately NOT used: a GitHub Pages address says nothing about the
   repository. Nothing leaves the site; while the question is open neither text is shown. No network, an error, a missing file, a timeout: "not active yet".
   Only on a local address (localhost, 127.0.0.1, [::1]) "?source=live" shows the live text for a preview. */
(function () {
  "use strict";
  var cfg = window.TSW_SITE;
  var nodes = document.querySelectorAll("[data-update-source]");

  function show(live) {
    for (var i = 0; i < nodes.length; i++) { nodes[i].hidden = (nodes[i].getAttribute("data-update-source") === "live") !== live; }
  }
  function hideAll() { for (var i = 0; i < nodes.length; i++) { nodes[i].hidden = true; } }

  function isLocal() {
    var h = window.location && window.location.hostname;
    return h === "localhost" || h === "127.0.0.1" || h === "[::1]" || h === "::1";
  }

  function exists(path, signal) {
    return window.fetch(path, { method: "HEAD", cache: "no-store", credentials: "same-origin", signal: signal }).then(function (r) { return !!r.ok; });
  }

  function start() {
    var mode = cfg.updateSourceLive;
    if (mode === true || mode === false) { show(mode); return; }
    if (isLocal() && /(^|[?&])source=live(&|$)/.test(window.location.search || "")) { show(true); return; }
    if (typeof window.fetch !== "function" || !Array.isArray(cfg.sourceProbe) || !cfg.sourceProbe.length) { show(false); return; }
    hideAll();
    var ctl = typeof AbortController === "function" ? new AbortController() : null;
    var timer = window.setTimeout(function () { if (ctl) { ctl.abort(); } }, 6000);
    Promise.all(cfg.sourceProbe.map(function (path) { return exists(path, ctl ? ctl.signal : undefined); }))
      .then(function (all) { show(all.every(Boolean)); }, function () { show(false); })
      .then(function () { window.clearTimeout(timer); });
  }

  start();
})();
