/* TSW-Forever web site: language, faction theme and the first-visit faction choice.
   No cookies, no server, no external requests. Choices live in localStorage only; if the browser refuses it (private mode, blocked storage, a file:// quirk) everything
   still works for the visit and a short notice says that the choice is not saved. */
(function () {
  "use strict";
  var cfg = window.TSW_SITE;
  var dict = window.TSW_I18N || {};
  var root = document.documentElement;
  var storageWarned = false;

  // ---- storage that may not exist ----------------------------------------------------------------------------------
  function load(key) {
    try { return window.localStorage.getItem(cfg.storagePrefix + key); } catch (e) { return null; }
  }
  function save(key, value) {
    try { window.localStorage.setItem(cfg.storagePrefix + key, value); return true; } catch (e) { return false; }
  }

  function toast(text) {
    var el = document.getElementById("toast");
    if (!el) { return; }
    el.textContent = text;
    el.hidden = false;
    window.setTimeout(function () { el.hidden = true; }, 7000);
  }
  function warnStorageOnce() {
    if (storageWarned) { return; }
    storageWarned = true;
    toast(text("msg.storage"));
  }

  // ---- language ---------------------------------------------------------------------------------------------------
  var current = cfg.defaultLanguage;

  function text(key) {
    var d = dict[current] || {};
    if (Object.prototype.hasOwnProperty.call(d, key)) { return d[key]; }
    var fallback = dict[cfg.defaultLanguage] || {};
    return Object.prototype.hasOwnProperty.call(fallback, key) ? fallback[key] : key;
  }

  function supported(code) { return cfg.languages.indexOf(code) !== -1; }

  /* The first supported language of the browser's list; Portuguese of any region maps to Brazilian Portuguese (the only variant of the launcher). */
  function detect() {
    var list = (navigator.languages && navigator.languages.length) ? navigator.languages : [navigator.language || ""];
    for (var i = 0; i < list.length; i++) {
      var tag = String(list[i] || "").toLowerCase();
      if (!tag) { continue; }
      var primary = tag.split("-")[0];
      if (primary === "pt") { return "pt-BR"; }
      if (supported(primary)) { return primary; }
    }
    return cfg.defaultLanguage;
  }

  function applyLanguage(code) {
    current = supported(code) ? code : cfg.defaultLanguage;
    root.setAttribute("lang", current);
    var nodes = document.querySelectorAll("[data-i18n]");
    for (var i = 0; i < nodes.length; i++) { nodes[i].textContent = text(nodes[i].getAttribute("data-i18n")); }
    var attrNodes = document.querySelectorAll("[data-i18n-attr]");
    for (var j = 0; j < attrNodes.length; j++) {
      var pairs = attrNodes[j].getAttribute("data-i18n-attr").split(";");
      for (var k = 0; k < pairs.length; k++) {
        var pair = pairs[k].split(":");
        if (pair.length === 2) { attrNodes[j].setAttribute(pair[0], text(pair[1])); }
      }
    }
    document.title = text("meta.title");
    var select = document.getElementById("lang");
    if (select) { select.value = current; }
    try { document.dispatchEvent(new CustomEvent("tsw-language")); } catch (e) { /* old browser: the download note keeps its last language */ }
  }

  // ---- faction theme -----------------------------------------------------------------------------------------------
  function validTheme(id) { return cfg.themes.indexOf(id) !== -1; }

  function applyTheme(id) {
    var theme = validTheme(id) ? id : cfg.defaultTheme;
    root.setAttribute("data-theme", theme);
    var chips = document.querySelectorAll(".chip[data-theme]");
    for (var i = 0; i < chips.length; i++) {
      chips[i].setAttribute("aria-pressed", chips[i].getAttribute("data-theme") === theme ? "true" : "false");
    }
    return theme;
  }

  function chooseTheme(id) {
    var theme = applyTheme(id);
    if (!save("theme", theme)) { warnStorageOnce(); }
    closeGate();
  }

  /* The faction choice is a modal dialog: while it is open the page behind it is darkened, blurred and inert (no clicks, no keyboard focus, hidden from screen readers),
     scrolling is locked and Tab stays inside the dialog. */
  var gate = document.getElementById("gate");
  var page = document.getElementById("page");
  var lastFocus = null;
  function setBackground(blocked) {
    document.body.classList.toggle("gated", blocked);
    if (!page) { return; }
    if (blocked) { page.setAttribute("inert", ""); page.setAttribute("aria-hidden", "true"); }
    else { page.removeAttribute("inert"); page.removeAttribute("aria-hidden"); }
  }
  function openGate() {
    lastFocus = document.activeElement;
    setBackground(true);
    gate.classList.add("open");
    var first = gate.querySelector(".faction");
    if (first) { first.focus(); }
  }
  function closeGate() {
    gate.classList.remove("open");
    setBackground(false);
    if (lastFocus && lastFocus !== document.body && lastFocus.focus) { lastFocus.focus(); }
  }
  function trapTab(e) {
    var items = gate.querySelectorAll("button");
    if (!items.length) { return; }
    var first = items[0], last = items[items.length - 1];
    if (!gate.contains(document.activeElement)) { first.focus(); e.preventDefault(); }
    else if (e.shiftKey && document.activeElement === first) { last.focus(); e.preventDefault(); }
    else if (!e.shiftKey && document.activeElement === last) { first.focus(); e.preventDefault(); }
  }

  // ---- copy buttons for the shell commands (the commands themselves are never translated) ----------------------------------
  function copyText(value) {
    if (navigator.clipboard && navigator.clipboard.writeText) { return navigator.clipboard.writeText(value); }
    return new Promise(function (resolve, reject) {
      var area = document.createElement("textarea");
      area.value = value; area.setAttribute("readonly", ""); area.style.position = "fixed"; area.style.opacity = "0";
      document.body.appendChild(area); area.select();
      var ok = false;
      try { ok = document.execCommand("copy"); } catch (e) { ok = false; }
      document.body.removeChild(area);
      if (ok) { resolve(); } else { reject(new Error("copy")); }
    });
  }
  function wireCopyButtons() {
    var buttons = document.querySelectorAll("[data-copy]");
    for (var i = 0; i < buttons.length; i++) {
      buttons[i].addEventListener("click", function (e) {
        var button = e.currentTarget;
        var code = button.parentNode.querySelector("pre code");
        if (!code) { return; }
        copyText(code.textContent.trim()).then(function () {
          button.textContent = text("ui.copied");
          window.setTimeout(function () { button.textContent = text("ui.copy"); }, 1600);
        }, function () { toast(text("ui.copyfail")); });
      });
    }
  }

  // ---- start ------------------------------------------------------------------------------------------------------
  function start() {
    var storedLang = load("lang");
    applyLanguage(storedLang && supported(storedLang) ? storedLang : detect());

    var storedTheme = load("theme");
    if (storedTheme && validTheme(storedTheme)) {
      applyTheme(storedTheme);
    } else {
      applyTheme(cfg.defaultTheme);
      openGate();
    }

    document.getElementById("lang").addEventListener("change", function (e) {
      applyLanguage(e.target.value);
      if (!save("lang", current)) { warnStorageOnce(); }
    });

    var chips = document.querySelectorAll(".chip[data-theme]");
    for (var c = 0; c < chips.length; c++) {
      chips[c].addEventListener("click", function (e) { chooseTheme(e.currentTarget.getAttribute("data-theme")); });
    }
    var picks = document.querySelectorAll("[data-pick]");
    for (var p = 0; p < picks.length; p++) {
      picks[p].addEventListener("click", function (e) { chooseTheme(e.currentTarget.getAttribute("data-pick")); });
    }
    /* Escape closes the choice for this visit only (nothing is stored, so it is offered again next time). */
    document.addEventListener("keydown", function (e) {
      if (!gate.classList.contains("open")) { return; }
      if (e.key === "Escape") { closeGate(); }
      else if (e.key === "Tab") { trapTab(e); }
    });
    wireCopyButtons();
  }

  window.TSW_TEXT = text;

  start();
})();
