// Behaviour test of website/js/source.js (does the page show the Flatpak update source as live?) without a browser. Run by scripts/test_website.py with bun.
// The decision must come from files that really answer, never from the host name.
const fs = require("fs"), path = require("path"), vm = require("vm");
const js = path.join(__dirname, "..", "website", "js");
const files = (cfgOverride) => fs.readFileSync(path.join(js, "config.js"), "utf8") + (cfgOverride || "") + "\n" + fs.readFileSync(path.join(js, "source.js"), "utf8");

async function run(name, { host = "glaubenskrieger.github.io", search = "", present = [], cfg = "", fetchImpl, noFetch = false }, expect) {
  const manual = { attrs: { "data-update-source": "manual" }, hidden: false, getAttribute(k) { return this.attrs[k]; } };
  const live = { attrs: { "data-update-source": "live" }, hidden: true, getAttribute(k) { return this.attrs[k]; } };
  const asked = [];
  const win = { location: { hostname: host, search }, AbortController, setTimeout, clearTimeout };
  if (!noFetch) win.fetch = fetchImpl || ((url, opt) => { asked.push(url + " " + opt.method); return Promise.resolve({ ok: present.includes(url) }); });
  win.window = win; win.document = { querySelectorAll: () => [manual, live] };
  vm.runInNewContext(files(cfg), win);
  const duringProbe = { manual: manual.hidden, live: live.hidden };
  await new Promise((r) => setTimeout(r, 40));
  const got = { manualHidden: manual.hidden, liveHidden: live.hidden };
  const ok = got.manualHidden === expect.manualHidden && got.liveHidden === expect.liveHidden && (expect.asked === undefined || asked.length === expect.asked);
  console.log(JSON.stringify({ name, got, asked, duringProbe, ok }));
  if (!ok) process.exit(1);
}
const ALL = ["repo/summary", "tsw-forever.flatpakrepo", "org.tswlauncher.Launcher.flatpakref"];
const LIVE = { manualHidden: true, liveHidden: false }, NOT = { manualHidden: false, liveHidden: true };

(async () => {
  await run("all three files answer -> live", { present: ALL }, { ...LIVE, asked: 3 });
  await run("same on a custom host (the host name is irrelevant)", { host: "example.org", present: ALL }, LIVE);
  await run("GitHub Pages host alone does NOT make it live (nothing answers)", { present: [] }, NOT);
  await run("repository missing", { present: ALL.slice(1) }, NOT);
  await run("repo summary only (no install files)", { present: ["repo/summary"] }, NOT);
  await run(".flatpakref missing", { present: ALL.slice(0, 2) }, NOT);
  await run("local preview: nothing exists -> not active", { host: "localhost", present: [] }, NOT);
  await run("local preview with ?source=live shows the live text (preview only)", { host: "localhost", search: "?source=live", present: [] }, LIVE);
  await run("?source=live is ignored on a public host", { host: "glaubenskrieger.github.io", search: "?source=live", present: [] }, NOT);
  await run("network error", { fetchImpl: () => Promise.reject(new TypeError("offline")) }, NOT);
  await run("no fetch support", { noFetch: true }, NOT);
  await run("forced true", { cfg: "\nwindow.TSW_SITE.updateSourceLive = true;", present: [] }, LIVE);
  await run("forced false", { cfg: "\nwindow.TSW_SITE.updateSourceLive = false;", present: ALL }, NOT);
  // while the question is open neither text is shown
  const pending = { fetchImpl: () => new Promise(() => {}) };
  const manual = { attrs: { "data-update-source": "manual" }, hidden: false, getAttribute(k) { return this.attrs[k]; } };
  const live = { attrs: { "data-update-source": "live" }, hidden: true, getAttribute(k) { return this.attrs[k]; } };
  const win = { location: { hostname: "x.github.io", search: "" }, AbortController, setTimeout, clearTimeout, fetch: pending.fetchImpl };
  win.window = win; win.document = { querySelectorAll: () => [manual, live] };
  vm.runInNewContext(files(), win);
  const both = manual.hidden && live.hidden;
  console.log(JSON.stringify({ name: "while the answer is pending neither text is shown", ok: both }));
  if (!both) process.exit(1);
  process.exit(0);
})();
