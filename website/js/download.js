/* Download buttons for Linux (Flatpak bundle) and Windows (installer): bound to the newest public STABLE release of the project's repository, never to a version written into this site.
   The page asks the GitHub API once (no cookies, no referrer, no credentials) for the latest release (GitHub's "latest" is never a draft or a pre-release) and accepts only
   - a tag of the form vMAJOR.MINOR.PATCH,
   - assets whose name has exactly one of these shapes, for the SAME version as the tag, and whose address is that repository's release download address for that tag:
       tsw-forever-<MAJOR.MINOR.PATCH>.flatpak               (Linux)
       tsw-forever-<MAJOR.MINOR.PATCH>-windows-setup.exe     (Windows)
       SHA256SUMS                                            (checksums, optional link)
   A button never points to a draft, a pre-release or a file that is not there:
   - found:    the button links straight to the file of the newest release;
   - no release has been published yet (the API answers 404): the button is switched off and the text says so;
   - any other problem (no network, rate limit, odd names, a release without that file): the button opens the list of releases, and the text says that the version could not be
     determined automatically.
   Without JavaScript the buttons are that list of releases. */
(function () {
  "use strict";
  var cfg = window.TSW_SITE;
  var repo = cfg.repo;
  var API = "https://api.github.com/repos/" + repo + "/releases/latest";
  var DOWNLOAD = "https://github.com/" + repo + "/releases/download/";
  var RELEASES = "https://github.com/" + repo + "/releases";
  var TAG = /^v(\d+\.\d+\.\d+)$/;
  var SHA = /^sha256:([0-9a-f]{64})$/;
  var PLATFORMS = {
    linux: /^tsw-forever-(\d+\.\d+\.\d+)\.flatpak$/,
    windows: /^tsw-forever-(\d+\.\d+\.\d+)-windows-setup\.exe$/
  };

  var parts = {};
  var state = {};

  function t(key) { return typeof window.TSW_TEXT === "function" ? window.TSW_TEXT(key) : key; }

  function each(list, fn) { for (var i = 0; i < list.length; i++) { fn(list[i]); } }

  function find(platform) {
    return {
      buttons: document.querySelectorAll(".js-dl-" + platform),
      status: document.getElementById(platform + "-status"),
      sumsBox: document.getElementById(platform + "-sums"),
      sha: document.getElementById(platform + "-sha"),
      sumsLink: document.getElementById(platform + "-sums-link")
    };
  }

  function enable(button, href) {
    button.removeAttribute("aria-disabled");
    if (button.classList && button.classList.remove) { button.classList.remove("is-disabled"); }
    button.setAttribute("href", href);
  }

  function disable(button) {
    button.removeAttribute("href");
    button.setAttribute("aria-disabled", "true");
    if (button.classList && button.classList.add) { button.classList.add("is-disabled"); }
  }

  function render(platform) {
    var p = parts[platform], s = state[platform];
    if (!p || !s || !p.status) { return; }
    if (s.kind === "found") {
      p.status.removeAttribute("data-i18n");
      p.status.textContent = t("dl.status.found").replace("{version}", s.version).replace("{size}", s.size);
    } else if (s.kind === "failed") {
      p.status.removeAttribute("data-i18n");
      p.status.textContent = t("dl.status.failed");
    } else if (s.kind === "none") {
      p.status.removeAttribute("data-i18n");
      p.status.textContent = t("dl.status.none");
    }
  }

  function renderAll() { for (var platform in PLATFORMS) { render(platform); } }

  /* The newest release as {version, tag, assets: {linux, windows}, sums}, or null when it is not an acceptable public release. */
  function pick(release) {
    if (!release || release.draft || release.prerelease || typeof release.tag_name !== "string") { return null; }
    var tag = TAG.exec(release.tag_name);
    if (!tag || !Array.isArray(release.assets)) { return null; }
    var found = { version: tag[1], tag: release.tag_name, assets: { linux: null, windows: null }, sums: null };
    for (var i = 0; i < release.assets.length; i++) {
      var a = release.assets[i];
      if (!a || typeof a.name !== "string" || typeof a.browser_download_url !== "string") { continue; }
      if (a.browser_download_url !== DOWNLOAD + release.tag_name + "/" + a.name) { continue; }
      if (a.name === "SHA256SUMS") { found.sums = a; }
      for (var platform in PLATFORMS) {
        var m = PLATFORMS[platform].exec(a.name);
        if (m && m[1] === tag[1]) { found.assets[platform] = a; }
      }
    }
    return found;
  }

  function applyFound(platform, release) {
    var p = parts[platform], asset = release.assets[platform];
    each(p.buttons, function (b) { enable(b, asset.browser_download_url); });
    var mb = typeof asset.size === "number" ? (asset.size / 1048576).toFixed(1) + " MB" : "";
    state[platform] = { kind: "found", version: release.version, size: mb };
    if (p.sumsBox) {
      var digest = SHA.exec(typeof asset.digest === "string" ? asset.digest : "");
      if (digest && p.sha) { p.sha.textContent = digest[1]; }
      if (release.sums && p.sumsLink) { p.sumsLink.setAttribute("href", release.sums.browser_download_url); }
      p.sumsBox.hidden = !(digest || release.sums);
      if (!digest && p.sha && p.sha.parentNode && p.sha.parentNode.firstChild) { p.sha.parentNode.firstChild.hidden = true; }
    }
  }

  function applyFailed(platform) {
    each(parts[platform].buttons, function (b) { enable(b, RELEASES); });
    state[platform] = { kind: "failed" };
  }

  function applyNone(platform) {
    each(parts[platform].buttons, function (b) { disable(b); });
    state[platform] = { kind: "none" };
  }

  function applyAnswer(release) {
    var picked = pick(release);
    for (var platform in PLATFORMS) {
      if (picked && picked.assets[platform]) { applyFound(platform, picked); } else { applyFailed(platform); }
    }
    renderAll();
  }

  function applyAll(fn) {
    for (var platform in PLATFORMS) { fn(platform); }
    renderAll();
  }

  function start() {
    for (var platform in PLATFORMS) { parts[platform] = find(platform); }
    if (typeof window.fetch !== "function") { return; }
    var ctl = typeof AbortController === "function" ? new AbortController() : null;
    var timer = window.setTimeout(function () { if (ctl) { ctl.abort(); } }, 8000);
    window.fetch(API, { headers: { Accept: "application/vnd.github+json" }, credentials: "omit", referrerPolicy: "no-referrer", signal: ctl ? ctl.signal : undefined })
      .then(function (r) {
        if (r.status === 404) { applyAll(applyNone); return null; }
        if (!r.ok) { throw new Error("status " + r.status); }
        return r.json().then(applyAnswer);
      })
      .catch(function () { applyAll(applyFailed); })
      .then(function () { window.clearTimeout(timer); });
  }

  document.addEventListener("tsw-language", renderAll);
  window.TSW_DOWNLOAD = { pick: pick };  /* for the tests of this site */
  start();
})();
