// Behaviour test of website/js/download.js without a browser: a small fake page, a fake fetch, and every kind of answer GitHub could give.
// Run by scripts/test_website.py with bun (skipped there if no JavaScript runtime exists). Prints one JSON object per case; exits 1 on the first wrong result.
const fs = require("fs"), path = require("path"), vm = require("vm");
const site = path.join(__dirname, "..", "website", "js");
const REPO = "Glaubenskrieger/TSW-Forever";
const BASE = `https://github.com/${REPO}/releases/download/`;
const FALLBACK = `https://github.com/${REPO}/releases`;

function asset(tag, name, extra) { return Object.assign({ name, size: 15728640, browser_download_url: `${BASE}${tag}/${name}` }, extra || {}); }
function release(tag, assets, extra) { return Object.assign({ tag_name: tag, draft: false, prerelease: false, assets }, extra || {}); }
const exe = (v) => `tsw-forever-${v}-windows-setup.exe`;

async function run(name, answer, expect) {
  const els = {};
  const el = (id) => (els[id] = els[id] || { attrs: {}, hidden: true, textContent: "", setAttribute(k, v) { this.attrs[k] = v; }, removeAttribute(k) { delete this.attrs[k]; }, parentNode: { firstChild: { hidden: false } } });
  const btn = () => ({ attrs: { href: FALLBACK }, setAttribute(k, v) { this.attrs[k] = v; }, removeAttribute(k) { delete this.attrs[k]; }, classList: { names: new Set(), add(n) { this.names.add(n); }, remove(n) { this.names.delete(n); } } });
  const buttons = { linux: [btn(), btn()], windows: [btn(), btn()] };
  const listeners = {};
  const win = {
    TSW_TEXT: (k) => (k === "dl.status.found" ? "found {version} {size}" : k),
    fetch: typeof answer === "function" ? answer : () => Promise.resolve(answer),
    AbortController, setTimeout, clearTimeout,
  };
  const doc = { querySelectorAll: (q) => buttons[q.replace(".js-dl-", "")] || [], getElementById: (id) => el(id), addEventListener: (e, f) => (listeners[e] = f) };
  win.window = win; win.document = doc;
  vm.runInNewContext(fs.readFileSync(path.join(site, "config.js"), "utf8") + "\n" + fs.readFileSync(path.join(site, "download.js"), "utf8"), win);
  await new Promise((r) => setTimeout(r, 30));
  const out = {};
  let ok = true;
  for (const p of ["linux", "windows"]) {
    const want = expect[p] || { kind: "failed" };
    const hrefs = buttons[p].map((b) => b.attrs.href);
    const disabled = buttons[p].map((b) => b.attrs["aria-disabled"] === "true" && b.attrs.href === undefined && b.classList.names.has("is-disabled"));
    const status = els[p + "-status"].textContent;
    let good;
    if (want.kind === "found") good = hrefs.every((h) => h === want.href) && !disabled.some(Boolean) && status.startsWith("found ") && status.includes(want.version);
    else if (want.kind === "none") good = disabled.every(Boolean) && status === "dl.status.none";
    else good = hrefs.every((h) => h === FALLBACK) && !disabled.some(Boolean) && status === "dl.status.failed";
    if (want.kind === "found" && want.sumsHidden !== undefined) good = good && els[p + "-sums"].hidden === want.sumsHidden;
    out[p] = { hrefs, disabled, status, sumsHidden: els[p + "-sums"].hidden, good };
    ok = ok && good;
  }
  console.log(JSON.stringify({ name, ok, ...out }));
  if (!ok) process.exit(1);
}
const json = (o, ok = true, status = 200) => ({ ok, status, json: () => Promise.resolve(o) });

(async () => {
  const sha = "sha256:" + "0f".repeat(32);
  const flat = (v) => `tsw-forever-${v}.flatpak`;
  const good = (v) => release("v" + v, [asset("v" + v, exe(v), { digest: sha }), asset("v" + v, "SHA256SUMS"), asset("v" + v, flat(v), { digest: sha }), asset("v" + v, `tsw-forever-${v}-src.tar.gz`)]);
  const found = (v) => ({ linux: { kind: "found", href: BASE + `v${v}/` + flat(v), version: v, sumsHidden: false }, windows: { kind: "found", href: BASE + `v${v}/` + exe(v), version: v, sumsHidden: false } });
  const failed = {}, none = { linux: { kind: "none" }, windows: { kind: "none" } };
  await run("current release: both files", json(good("0.4.1")), found("0.4.1"));
  // no version is written into the site: any future number is taken from the answer
  await run("future release 12.0.3", json(good("12.0.3")), found("12.0.3"));
  await run("no digest, sums file only", json(release("v1.0.0", [asset("v1.0.0", exe("1.0.0")), asset("v1.0.0", flat("1.0.0")), asset("v1.0.0", "SHA256SUMS")])), found("1.0.0"));
  await run("no release published yet (404): both buttons are off", json({ message: "Not Found" }, false, 404), none);
  await run("rate limited (403): the release list", json({ message: "rate limit" }, false, 403), failed);
  await run("server error (500)", json({}, false, 500), failed);
  await run("network error", () => Promise.reject(new TypeError("offline")), failed);
  await run("answer is not JSON", { ok: true, status: 200, json: () => Promise.reject(new Error("bad json")) }, failed);
  await run("release without a Linux file: only Windows is found", json(release("v0.4.1", [asset("v0.4.1", exe("0.4.1"), { digest: sha }), asset("v0.4.1", "SHA256SUMS")])),
    { linux: { kind: "failed" }, windows: { kind: "found", href: BASE + "v0.4.1/" + exe("0.4.1"), version: "0.4.1" } });
  await run("release without a Windows file: only Linux is found", json(release("v0.4.1", [asset("v0.4.1", flat("0.4.1"), { digest: sha }), asset("v0.4.1", "SHA256SUMS")])),
    { linux: { kind: "found", href: BASE + "v0.4.1/" + flat("0.4.1"), version: "0.4.1" }, windows: { kind: "failed" } });
  await run("release without assets", json(release("v0.4.1", [])), failed);
  await run("pre-release", json(release("v0.5.0", good("0.5.0").assets, { prerelease: true })), failed);
  await run("draft is never linked", json(release("v0.5.0", good("0.5.0").assets, { draft: true })), failed);
  await run("tag is not vX.Y.Z", json(release("v0.4", good("0.4.0").assets)), failed);
  await run("tag with suffix", json(release("v0.4.0-rc1", good("0.4.0").assets)), failed);
  await run("files of another version than the tag", json(release("v0.4.1", [asset("v0.4.1", exe("0.3.0")), asset("v0.4.1", flat("0.3.0"))])), failed);
  await run("names with extra text", json(release("v0.4.1", [asset("v0.4.1", "tsw-forever-0.4.1-windows-setup.exe.zip"), asset("v0.4.1", "x-tsw-forever-0.4.1-windows-setup.exe"), asset("v0.4.1", "tsw-forever-0.4.1.flatpak.sha256"),
    asset("v0.4.1", "tsw-forever-0.4.1-testing.flatpak")])), failed);
  const foreign = good("0.4.1"); foreign.assets[0].browser_download_url = "https://evil.example/" + exe("0.4.1"); foreign.assets[2].browser_download_url = "https://evil.example/" + flat("0.4.1");
  await run("foreign download address", json(foreign), failed);
  const otherRepo = good("0.4.1"); otherRepo.assets[0].browser_download_url = `https://github.com/Glaubenskrieger/TSW-Forever-Dev/releases/download/v0.4.1/${exe("0.4.1")}`;
  otherRepo.assets[2].browser_download_url = `https://github.com/Glaubenskrieger/TSW-Forever-Dev/releases/download/v0.4.1/${flat("0.4.1")}`;
  await run("private dev repository address", json(otherRepo), failed);
  const otherTag = good("0.4.1"); otherTag.assets[0].browser_download_url = `${BASE}v9.9.9/${exe("0.4.1")}`; otherTag.assets[2].browser_download_url = `${BASE}v9.9.9/${flat("0.4.1")}`;
  await run("address of another tag", json(otherTag), failed);
  const badDigest = good("0.4.1"); badDigest.assets[0].digest = "sha256:zz"; badDigest.assets[2].digest = "sha256:zz";
  await run("malformed digest is not shown", json(badDigest), { linux: { kind: "found", href: BASE + "v0.4.1/" + flat("0.4.1"), version: "0.4.1" }, windows: { kind: "found", href: BASE + "v0.4.1/" + exe("0.4.1"), version: "0.4.1" } });
  await run("null body", json(null), failed);
})();
