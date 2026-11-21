// Behaviour test of website/js/download.js without a browser: a small fake page, a fake fetch, and every kind of answer GitHub could give.
// Run by scripts/test_website.py with bun (skipped there if no JavaScript runtime exists). Prints one JSON object per case; exits 1 on the first wrong result.
const fs = require("fs"), path = require("path"), vm = require("vm");
const site = path.join(__dirname, "..", "website", "js");
const REPO = "Glaubenskrieger/TSW-Forever";
const BASE = `https://github.com/${REPO}/releases/download/`;
const FALLBACK = `https://github.com/${REPO}/releases/latest`;

function asset(tag, name, extra) { return Object.assign({ name, size: 15728640, browser_download_url: `${BASE}${tag}/${name}` }, extra || {}); }
function release(tag, assets, extra) { return Object.assign({ tag_name: tag, draft: false, prerelease: false, assets }, extra || {}); }
const exe = (v) => `tsw-forever-${v}-windows-setup.exe`;

async function run(name, answer, expect) {
  const els = {};
  const el = (id) => (els[id] = els[id] || { attrs: {}, hidden: true, textContent: "", setAttribute(k, v) { this.attrs[k] = v; }, removeAttribute(k) { delete this.attrs[k]; }, parentNode: { firstChild: { hidden: false } } });
  const buttons = [el("b1"), el("b2")];
  for (const b of buttons) b.attrs.href = FALLBACK;
  const listeners = {};
  const win = {
    TSW_TEXT: (k) => (k === "dl.win.status.found" ? "found {version} {size}" : k),
    fetch: typeof answer === "function" ? answer : () => Promise.resolve(answer),
    AbortController, setTimeout, clearTimeout,
  };
  const doc = { querySelectorAll: (q) => (q === ".js-win-dl" ? buttons : []), getElementById: (id) => el(id), addEventListener: (e, f) => (listeners[e] = f) };
  win.window = win; win.document = doc;
  vm.runInNewContext(fs.readFileSync(path.join(site, "config.js"), "utf8") + "\n" + fs.readFileSync(path.join(site, "download.js"), "utf8"), win);
  await new Promise((r) => setTimeout(r, 30));
  const hrefs = buttons.map((b) => b.attrs.href);
  const status = els["win-status"].textContent;
  const ok = expect.href ? hrefs.every((h) => h === expect.href) : hrefs.every((h) => h === FALLBACK);
  const okStatus = expect.status ? status.includes(expect.status) : (expect.href ? status.startsWith("found ") : status === "dl.win.status.failed");
  const sums = els["win-sums"].hidden;
  const result = { name, hrefs, status, sumsHidden: sums, ok: ok && okStatus && (expect.sumsHidden === undefined || sums === expect.sumsHidden) };
  console.log(JSON.stringify(result));
  if (!result.ok) process.exit(1);
}
const json = (o, ok = true, status = 200) => ({ ok, status, json: () => Promise.resolve(o) });

(async () => {
  const sha = "sha256:" + "0f".repeat(32);
  const good = (v) => release("v" + v, [asset("v" + v, exe(v), { digest: sha }), asset("v" + v, "SHA256SUMS"), asset("v" + v, `tsw-forever-${v}.flatpak`)]);
  await run("current release", json(good("0.4.0")), { href: BASE + "v0.4.0/" + exe("0.4.0"), status: "0.4.0", sumsHidden: false });
  // no version is written into the site: any future number is taken from the answer
  await run("future release 12.0.3", json(good("12.0.3")), { href: BASE + "v12.0.3/" + exe("12.0.3"), status: "12.0.3" });
  await run("no digest, sums file only", json(release("v1.0.0", [asset("v1.0.0", exe("1.0.0")), asset("v1.0.0", "SHA256SUMS")])), { href: BASE + "v1.0.0/" + exe("1.0.0"), sumsHidden: false });
  await run("no release yet (404)", json({ message: "Not Found" }, false, 404), {});
  await run("rate limited (403)", json({ message: "rate limit" }, false, 403), {});
  await run("network error", () => Promise.reject(new TypeError("offline")), {});
  await run("answer is not JSON", { ok: true, status: 200, json: () => Promise.reject(new Error("bad json")) }, {});
  await run("release without an installer", json(release("v0.4.0", [asset("v0.4.0", "tsw-forever-0.4.0.flatpak"), asset("v0.4.0", "SHA256SUMS")])), {});
  await run("release without assets", json(release("v0.4.0", [])), {});
  await run("pre-release", json(release("v0.5.0", good("0.5.0").assets, { prerelease: true })), {});
  await run("draft", json(release("v0.5.0", good("0.5.0").assets, { draft: true })), {});
  await run("tag is not vX.Y.Z", json(release("v0.4", good("0.4.0").assets)), {});
  await run("tag with suffix", json(release("v0.4.0-rc1", good("0.4.0").assets)), {});
  await run("installer of another version than the tag", json(release("v0.4.0", [asset("v0.4.0", exe("0.3.0"))])), {});
  await run("installer name with extra text", json(release("v0.4.0", [asset("v0.4.0", "tsw-forever-0.4.0-windows-setup.exe.zip"), asset("v0.4.0", "x-tsw-forever-0.4.0-windows-setup.exe")])), {});
  const foreign = good("0.4.0"); foreign.assets[0].browser_download_url = "https://evil.example/" + exe("0.4.0");
  await run("foreign download address", json(foreign), {});
  const otherRepo = good("0.4.0"); otherRepo.assets[0].browser_download_url = `https://github.com/Glaubenskrieger/TSW-Forever-Dev/releases/download/v0.4.0/${exe("0.4.0")}`;
  await run("private dev repository address", json(otherRepo), {});
  const otherTag = good("0.4.0"); otherTag.assets[0].browser_download_url = `${BASE}v9.9.9/${exe("0.4.0")}`;
  await run("address of another tag", json(otherTag), {});
  const badDigest = good("0.4.0"); badDigest.assets[0].digest = "sha256:zz";
  await run("malformed digest is not shown", json(badDigest), { href: BASE + "v0.4.0/" + exe("0.4.0") });
  await run("null body", json(null), {});
})();
