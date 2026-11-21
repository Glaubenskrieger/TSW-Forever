/* Windows download link: bound to the newest public STABLE release of the project's repository, never to a version written into this site.
   The page asks the GitHub API once (no cookies, no referrer, no credentials) for the latest release (GitHub's "latest" is never a draft or pre-release), and accepts only
   - a tag of the form vMAJOR.MINOR.PATCH,
   - an asset named exactly tsw-forever-<MAJOR.MINOR.PATCH>-windows-setup.exe whose address is that repository's release download address for that tag.
   Anything else (no network, rate limit, no release yet, no installer yet, odd names) leaves the button on the release page. Without JavaScript the button is that link too. */
(function () {
  "use strict";
  var cfg = window.TSW_SITE;
  var repo = cfg.repo;
  var API = "https://api.github.com/repos/" + repo + "/releases/latest";
  var DOWNLOAD = "https://github.com/" + repo + "/releases/download/";
  var TAG = /^v(\d+\.\d+\.\d+)$/;
  var INSTALLER = /^tsw-forever-(\d+\.\d+\.\d+)-windows-setup\.exe$/;
  var SHA = /^sha256:([0-9a-f]{64})$/;

  var buttons = document.querySelectorAll(".js-win-dl");
  var status = document.getElementById("win-status");
  var sumsBox = document.getElementById("win-sums");
  var shaEl = document.getElementById("win-sha");
  var sumsLink = document.getElementById("win-sums-link");
  var state = { kind: "default" };

  function t(key) { return typeof window.TSW_TEXT === "function" ? window.TSW_TEXT(key) : key; }

  function render() {
    if (!status) { return; }
    if (state.kind === "found") {
      status.removeAttribute("data-i18n");
      status.textContent = t("dl.win.status.found").replace("{version}", state.version).replace("{size}", state.size);
    } else if (state.kind === "failed") {
      status.removeAttribute("data-i18n");
      status.textContent = t("dl.win.status.failed");
    }
  }

  function pick(release) {
    if (!release || release.draft || release.prerelease || typeof release.tag_name !== "string") { return null; }
    var tag = TAG.exec(release.tag_name);
    if (!tag || !Array.isArray(release.assets)) { return null; }
    var installer = null, sums = null;
    for (var i = 0; i < release.assets.length; i++) {
      var a = release.assets[i];
      if (!a || typeof a.name !== "string" || typeof a.browser_download_url !== "string") { continue; }
      var wanted = DOWNLOAD + release.tag_name + "/" + a.name;
      if (a.browser_download_url !== wanted) { continue; }
      var m = INSTALLER.exec(a.name);
      if (m && m[1] === tag[1]) { installer = a; }
      if (a.name === "SHA256SUMS") { sums = a; }
    }
    return installer ? { version: tag[1], installer: installer, sums: sums } : null;
  }

  function apply(found) {
    for (var i = 0; i < buttons.length; i++) { buttons[i].setAttribute("href", found.installer.browser_download_url); }
    var mb = typeof found.installer.size === "number" ? (found.installer.size / 1048576).toFixed(1) + " MB" : "";
    state = { kind: "found", version: found.version, size: mb };
    if (sumsBox) {
      var digest = SHA.exec(typeof found.installer.digest === "string" ? found.installer.digest : "");
      if (digest && shaEl) { shaEl.textContent = digest[1]; }
      if (found.sums && sumsLink) { sumsLink.setAttribute("href", found.sums.browser_download_url); }
      sumsBox.hidden = !(digest || found.sums);
      if (!digest && shaEl) { shaEl.parentNode.firstChild.hidden = true; }
    }
    render();
  }

  function fail() { state = { kind: "failed" }; render(); }

  function start() {
    if (typeof window.fetch !== "function") { return; }
    var ctl = typeof AbortController === "function" ? new AbortController() : null;
    var timer = window.setTimeout(function () { if (ctl) { ctl.abort(); } }, 8000);
    window.fetch(API, { headers: { Accept: "application/vnd.github+json" }, credentials: "omit", referrerPolicy: "no-referrer", signal: ctl ? ctl.signal : undefined })
      .then(function (r) { if (!r.ok) { throw new Error("status " + r.status); } return r.json(); })
      .then(function (json) { var found = pick(json); if (found) { apply(found); } else { fail(); } })
      .catch(fail)
      .then(function () { window.clearTimeout(timer); });
  }

  document.addEventListener("tsw-language", render);
  window.TSW_DOWNLOAD = { pick: pick };  /* for the tests of this site */
  start();
})();
