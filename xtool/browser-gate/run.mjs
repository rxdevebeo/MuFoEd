#!/usr/bin/env node
/**
 * Pinned Chromium-family CDP harness for F07/F20 browser acceptance (R09).
 *
 * Exit: 0 PASS · 1 FAIL · 3 BLOCKED (browser/CDP missing)
 */

import { spawn } from "node:child_process";
import { createHash } from "node:crypto";
import {
  existsSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import net from "node:net";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { setTimeout as sleep } from "node:timers/promises";

const EXIT_OK = 0;
const EXIT_FAIL = 1;
const EXIT_BLOCKED = 3;
const HERE = dirname(fileURLToPath(import.meta.url));
const REPO = resolve(HERE, "../..");

function parseArgs(argv) {
  const out = {
    url: null,
    browser: process.env.STRICTLIB_BROWSER || null,
    report: null,
    scenarios: ["f20-matrix", "f07-face"],
    expectedCarlito: null,
  };
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg === "--url") out.url = argv[++i];
    else if (arg === "--browser") out.browser = argv[++i];
    else if (arg === "--report") out.report = argv[++i];
    else if (arg === "--scenarios") out.scenarios = argv[++i].split(",");
    else if (arg === "--expected-carlito-hash") out.expectedCarlito = argv[++i];
  }
  return out;
}

function candidateBrowsers(explicit) {
  const list = [];
  if (explicit) list.push(explicit);
  if (process.env.STRICTLIB_BROWSER) list.push(process.env.STRICTLIB_BROWSER);
  if (process.platform === "win32") {
    list.push(
      "C:\\Program Files (x86)\\Microsoft\\Edge\\Application\\msedge.exe",
      "C:\\Program Files\\Microsoft\\Edge\\Application\\msedge.exe",
      "C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe",
      "C:\\Program Files (x86)\\Google\\Chrome\\Application\\chrome.exe",
    );
  } else if (process.platform === "darwin") {
    list.push(
      "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
      "/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge",
    );
  } else {
    list.push(
      "/usr/bin/google-chrome",
      "/usr/bin/google-chrome-stable",
      "/usr/bin/chromium",
      "/usr/bin/chromium-browser",
      "/usr/bin/microsoft-edge",
      "/usr/bin/microsoft-edge-stable",
    );
  }
  return list;
}

function resolveBrowser(explicit) {
  for (const path of candidateBrowsers(explicit)) {
    if (path && existsSync(path)) return path;
  }
  return null;
}

async function freePort() {
  return await new Promise((resolvePort, reject) => {
    const server = net.createServer();
    server.listen(0, "127.0.0.1", () => {
      const { port } = server.address();
      server.close((error) => (error ? reject(error) : resolvePort(port)));
    });
    server.on("error", reject);
  });
}

async function waitForJson(url, attempts = 80) {
  for (let i = 0; i < attempts; i += 1) {
    try {
      const response = await fetch(url);
      if (response.ok) return await response.json();
    } catch {
      // retry
    }
    await sleep(100);
  }
  throw new Error(`CDP endpoint not ready: ${url}`);
}

function makeSession(ws) {
  let nextId = 1;
  const pending = new Map();
  ws.addEventListener("message", (event) => {
    const message = JSON.parse(String(event.data));
    if (message.id != null && pending.has(message.id)) {
      const { resolve, reject } = pending.get(message.id);
      pending.delete(message.id);
      if (message.error) reject(new Error(JSON.stringify(message.error)));
      else resolve(message.result);
    }
  });
  const send = (method, params = {}, sessionId) => {
    const id = nextId++;
    const payload = { id, method, params };
    if (sessionId) payload.sessionId = sessionId;
    return new Promise((resolve, reject) => {
      pending.set(id, { resolve, reject });
      ws.send(JSON.stringify(payload));
    });
  };
  return { send, pending, nextIdRef: () => nextId, setNextId: (value) => { nextId = value; } };
}

async function launchBrowser(browserPath) {
  const port = await freePort();
  const profile = mkdtempSync(join(tmpdir(), "strictlib-browser-gate-"));
  const child = spawn(
    browserPath,
    [
      `--remote-debugging-port=${port}`,
      `--user-data-dir=${profile}`,
      "--headless=new",
      "--disable-gpu",
      "--no-first-run",
      "--no-default-browser-check",
      "--disable-extensions",
      "about:blank",
    ],
    { stdio: ["ignore", "pipe", "pipe"], windowsHide: true },
  );
  let stderr = "";
  child.stderr.on("data", (chunk) => {
    stderr += String(chunk);
  });
  try {
    const version = await waitForJson(`http://127.0.0.1:${port}/json/version`);
    const ws = new WebSocket(version.webSocketDebuggerUrl);
    await new Promise((resolve, reject) => {
      ws.addEventListener("open", resolve, { once: true });
      ws.addEventListener("error", reject, { once: true });
    });
    const root = makeSession(ws);
    await root.send("Target.setDiscoverTargets", { discover: true });
    const { targetId } = await root.send("Target.createTarget", {
      url: "about:blank",
    });
    const { sessionId } = await root.send("Target.attachToTarget", {
      targetId,
      flatten: true,
    });
    const session = {
      async send(method, params = {}) {
        return root.send(method, params, sessionId);
      },
      async evaluate(expression, awaitPromise = true) {
        const result = await session.send("Runtime.evaluate", {
          expression,
          awaitPromise,
          returnByValue: true,
          userGesture: true,
        });
        if (result.exceptionDetails) {
          throw new Error(JSON.stringify(result.exceptionDetails));
        }
        return result.result?.value;
      },
    };
    await session.send("Page.enable");
    await session.send("Runtime.enable");
    return {
      version,
      browserPath,
      session,
      stderr: () => stderr,
      async close() {
        try {
          ws.close();
        } catch {
          // ignore
        }
        child.kill();
        await sleep(150);
        try {
          rmSync(profile, { recursive: true, force: true });
        } catch {
          // ignore
        }
      },
    };
  } catch (error) {
    child.kill();
    try {
      rmSync(profile, { recursive: true, force: true });
    } catch {
      // ignore
    }
    const err = new Error(
      `BLOCKED: CDP attach failed: ${error instanceof Error ? error.message : error}\n${stderr}`,
    );
    err.code = EXIT_BLOCKED;
    throw err;
  }
}

async function openShell(session, baseUrl) {
  await session.send("Page.navigate", { url: `${baseUrl}/` });
  for (let i = 0; i < 100; i += 1) {
    const ready = await session.evaluate(`!!document.getElementById('menu-list')`);
    if (ready) {
      // wait for corpora/documents fetch
      const docs = await session.evaluate(
        `document.querySelectorAll('#menu-list button').length`,
      );
      if (docs > 0 || i > 20) return;
    }
    await sleep(100);
  }
  throw new Error("viewer shell did not load");
}

async function openDocument(session, name) {
  await session.evaluate(`(async () => {
    const name = ${JSON.stringify(name)};
    const menuButton = document.getElementById('menu-button');
    if (menuButton && menuButton.getAttribute('aria-expanded') !== 'true') {
      menuButton.click();
    }
    await new Promise((r) => setTimeout(r, 30));
    const hit = [...document.querySelectorAll('#menu-list button')]
      .find((button) => (button.querySelector('.name')?.textContent || button.textContent) === name
        || button.textContent.includes(name));
    if (!hit) throw new Error('menu missing ' + name);
    hit.click();
  })()`);
  for (let i = 0; i < 120; i += 1) {
    const state = await session.evaluate(`(() => {
      const name = ${JSON.stringify(name)};
      const docName = document.getElementById('doc-name');
      const status = document.getElementById('status');
      if (!docName || docName.textContent !== name) return null;
      if (status && status.textContent.includes('rendering')) return null;
      const losses = document.getElementById('losses');
      const list = document.getElementById('losses-list');
      const pages = document.getElementById('pages');
      const error = document.getElementById('error');
      return {
        outcome: losses?.dataset?.outcome || null,
        sidecar: losses?.dataset?.sidecar || null,
        lossesHidden: !!losses?.hidden,
        listHidden: !!list?.hidden,
        listText: list ? list.textContent : '',
        toggleText: document.getElementById('losses-toggle')?.textContent || '',
        pageCount: pages ? pages.querySelectorAll('.page').length : 0,
        hasError: !!error,
        errorText: error ? error.textContent : '',
        status: status ? status.textContent : '',
      };
    })()`);
    if (state) {
      const api = await session.evaluate(`(async () => {
        const name = ${JSON.stringify(name)};
        const response = await fetch('/api/document?name=' + encodeURIComponent(name));
        if (!response.ok) throw new Error('api ' + response.status);
        return await response.json();
      })()`);
      return { dom: state, api };
    }
    await sleep(100);
  }
  throw new Error(`document ${name} did not settle`);
}

async function keyboardExpand(session) {
  await session.evaluate(`(() => {
    const toggle = document.getElementById('losses-toggle');
    const list = document.getElementById('losses-list');
    list.hidden = true;
    toggle.setAttribute('aria-expanded', 'false');
    toggle.focus();
    const event = new KeyboardEvent('keydown', {
      key: 'Enter',
      code: 'Enter',
      bubbles: true,
      cancelable: true,
    });
    toggle.dispatchEvent(event);
  })()`);
  const state = await session.evaluate(`({
    hidden: document.getElementById('losses-list').hidden,
    expanded: document.getElementById('losses-toggle').getAttribute('aria-expanded'),
    text: document.getElementById('losses-list').textContent,
  })`);
  if (state.hidden || state.expanded !== "true") {
    throw new Error(`keyboard expand failed: ${JSON.stringify(state)}`);
  }
  return state;
}

async function scenarioF20(session, baseUrl) {
  const results = [];
  await openShell(session, baseUrl);

  {
    const { dom, api } = await openDocument(session, "vml-loss.docx");
    if (api.pipeline?.outcome !== "degraded") {
      throw new Error(`degraded API expected, got ${api.pipeline?.outcome}`);
    }
    if (api.pipeline?.sidecar !== "rejected") {
      throw new Error(`rejected sidecar expected, got ${api.pipeline?.sidecar}`);
    }
    if (dom.outcome !== "degraded" || dom.sidecar !== "rejected") {
      throw new Error(`DOM surface wrong: ${JSON.stringify(dom)}`);
    }
    if (!/VML|dropped|loss/i.test(dom.toggleText + dom.listText)) {
      throw new Error(`loss reason missing after JS: ${dom.toggleText}`);
    }
    const stages = (api.pipeline.stages || []).map((s) => `${s.stage}:${s.status}`);
    if (!stages.includes("convert:not_run") || !stages.includes("write:not_run")) {
      throw new Error(`not_run missing: ${stages.join(",")}`);
    }
    const expanded = await keyboardExpand(session);
    if (!expanded.text.includes("not_run")) {
      throw new Error("not_run stages not visible after keyboard expand");
    }
    results.push({ id: "f20-degraded-rejected-sidecar", ok: true });
  }

  {
    const { dom, api } = await openDocument(session, "plain.docx");
    if (api.pipeline?.outcome !== "clean") {
      throw new Error(`clean API expected, got ${api.pipeline?.outcome}`);
    }
    if (dom.outcome !== "clean") {
      throw new Error(`DOM clean expected, got ${dom.outcome}`);
    }
    const expanded = await keyboardExpand(session);
    if (!expanded.text.includes("not_run")) {
      throw new Error("clean view must expose not_run after keyboard expand");
    }
    results.push({ id: "f20-clean", ok: true, sidecar: api.pipeline?.sidecar });
  }

  {
    const { dom, api } = await openDocument(session, "sidecar-many.docx");
    if (api.pipeline?.sidecar !== "matched") {
      throw new Error(`matched sidecar expected, got ${api.pipeline?.sidecar}`);
    }
    if (dom.sidecar !== "matched") {
      throw new Error(`DOM sidecar not matched: ${dom.sidecar}`);
    }
    const issueCount = (api.pipeline?.issues || []).length;
    if (issueCount < 5) {
      throw new Error(`many issues expected, got ${issueCount}`);
    }
    const listText = await session.evaluate(
      `document.getElementById('losses-list').textContent`,
    );
    if (!listText.includes("<script>alert(1)</script>")) {
      throw new Error(`escaping failed: ${listText}`);
    }
    const injected = await session.evaluate(
      `document.querySelector('#losses-list script') !== null`,
    );
    if (injected) throw new Error("detail HTML executed as markup");
    results.push({
      id: "f20-matched-sidecar",
      ok: true,
      issueCount,
      also: "f20-clean-matched-sidecar-escape-many",
    });
  }

  {
    const { dom, api } = await openDocument(session, "broken.docx");
    if (api.pipeline?.outcome !== "failed") {
      throw new Error(`failed expected, got ${api.pipeline?.outcome}`);
    }
    if (dom.outcome !== "failed") {
      throw new Error(`DOM failed expected, got ${dom.outcome}`);
    }
    if (!dom.hasError && !/could not|failed|zip|open|damaged/i.test(dom.errorText + dom.toggleText)) {
      throw new Error(`failed open not visible: ${JSON.stringify(dom)}`);
    }
    results.push({ id: "f20-failed-open", ok: true });
  }

  return results;
}

async function scenarioF07(session, baseUrl, expectedCarlito) {
  await openShell(session, baseUrl);
  await openDocument(session, "face.docx");
  await session.evaluate(
    `document.fonts ? document.fonts.ready : Promise.resolve()`,
  );
  const probe = await session.evaluate(`(async () => {
    await (document.fonts ? document.fonts.ready : Promise.resolve());
    const page = document.querySelector('#pages .page');
    if (!page) return { ok: false, reason: 'no page' };
    const html = page.innerHTML;
    const hashMatch = html.match(/resource_hash=([0-9a-f]{64})/);
    const dataMatch = html.match(/url\\(\"data:font\\/ttf;base64,([A-Za-z0-9+/=]+)\"\\)/);
    if (!hashMatch || !dataMatch) {
      return { ok: false, reason: 'missing face payload', head: html.slice(0, 240) };
    }
    const binary = atob(dataMatch[1]);
    const bytes = new Uint8Array(binary.length);
    for (let i = 0; i < binary.length; i++) bytes[i] = binary.charCodeAt(i);
    const digest = await crypto.subtle.digest('SHA-256', bytes);
    const hash = [...new Uint8Array(digest)].map((b) => b.toString(16).padStart(2, '0')).join('');
    const carlitoLoaded = [...document.fonts].some((f) =>
      f.family.replace(/["']/g, '') === 'Carlito' && f.status === 'loaded');
    const fontsCheck = document.fonts.check('16px Carlito');
    const textEl = page.querySelector('svg text');
    let selected = '';
    if (textEl) {
      const range = document.createRange();
      range.selectNodeContents(textEl);
      const sel = window.getSelection();
      sel.removeAllRanges();
      sel.addRange(range);
      selected = String(sel);
    }
    const userSelect = getComputedStyle(page).userSelect || getComputedStyle(page).webkitUserSelect;
    return {
      ok: true,
      commentHash: hashMatch[1],
      bytesHash: hash,
      carlitoLoaded,
      fontsCheck,
      selected,
      userSelect,
      textContent: textEl ? textEl.textContent : null,
      faces: [...document.fonts].map((f) => ({ family: f.family, status: f.status })),
    };
  })()`);
  if (!probe.ok) throw new Error(`F07 probe failed: ${JSON.stringify(probe)}`);
  if (probe.commentHash !== probe.bytesHash) {
    throw new Error(`hash comment ${probe.commentHash} != bytes ${probe.bytesHash}`);
  }
  if (probe.bytesHash !== expectedCarlito) {
    throw new Error(`bytes ${probe.bytesHash} != R05 Carlito ${expectedCarlito}`);
  }
  if (!probe.carlitoLoaded && !probe.fontsCheck) {
    throw new Error(`Carlito not loaded: ${JSON.stringify(probe.faces)}`);
  }
  if (!probe.textContent) throw new Error("no selectable SVG text");
  if (probe.userSelect === "none") throw new Error("user-select: none on page");
  return [
    {
      id: "f07-browser-face-load",
      ok: true,
      resource_hash: probe.bytesHash,
      carlitoLoaded: probe.carlitoLoaded,
      fontsCheck: probe.fontsCheck,
      selected: probe.selected,
      userSelect: probe.userSelect,
    },
  ];
}

function defaultCarlitoHash() {
  const path = join(
    REPO,
    "strict-ooxml-render-svg/assets/fonts/carlito/Carlito-Regular.ttf",
  );
  return createHash("sha256").update(readFileSync(path)).digest("hex");
}

async function main() {
  const args = parseArgs(process.argv.slice(2));
  if (!args.url) {
    console.error("error: --url is required");
    process.exit(EXIT_FAIL);
  }
  const browserPath = resolveBrowser(args.browser);
  if (!browserPath) {
    const report = {
      status: "BLOCKED",
      reason: "browser runtime missing",
      candidates: candidateBrowsers(args.browser),
    };
    if (args.report) writeFileSync(args.report, JSON.stringify(report, null, 2));
    console.error("BLOCKED: no Chromium-family browser found");
    process.exit(EXIT_BLOCKED);
  }

  const expectedCarlito = args.expectedCarlito || defaultCarlitoHash();
  let browser;
  try {
    browser = await launchBrowser(browserPath);
  } catch (error) {
    const report = {
      status: "BLOCKED",
      reason: error instanceof Error ? error.message : String(error),
      browserPath,
    };
    if (args.report) writeFileSync(args.report, JSON.stringify(report, null, 2));
    console.error(report.reason);
    process.exit(EXIT_BLOCKED);
  }

  const baseUrl = args.url.replace(/\/$/, "");
  const all = [];
  try {
    if (args.scenarios.includes("f20-matrix")) {
      all.push(...(await scenarioF20(browser.session, baseUrl)));
    }
    if (args.scenarios.includes("f07-face")) {
      all.push(...(await scenarioF07(browser.session, baseUrl, expectedCarlito)));
    }
    const report = {
      status: "PASS",
      browserPath,
      browserVersion: browser.version,
      expectedCarlito,
      scenarios: all,
    };
    if (args.report) writeFileSync(args.report, JSON.stringify(report, null, 2));
    console.log(JSON.stringify({ status: "PASS", count: all.length }));
    process.exit(EXIT_OK);
  } catch (error) {
    const report = {
      status: "FAIL",
      browserPath,
      browserVersion: browser.version,
      expectedCarlito,
      scenarios: all,
      error: error instanceof Error ? error.message : String(error),
      stderr: browser.stderr(),
    };
    if (args.report) writeFileSync(args.report, JSON.stringify(report, null, 2));
    console.error(report.error);
    process.exit(EXIT_FAIL);
  } finally {
    await browser.close();
  }
}

main().catch((error) => {
  console.error(error);
  process.exit(EXIT_FAIL);
});
