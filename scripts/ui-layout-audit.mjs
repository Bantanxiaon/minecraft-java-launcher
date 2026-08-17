// 真实生产 Tauri DOM 布局审计：WebView2 CDP 对 release EXE 逐页执行
// getBoundingClientRect 收集（跳过父子包含的合法重叠）。
import { spawn, execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";

const REPO = process.cwd();
const PORT = 9223;
const EXE = path.join(REPO, "src-tauri", "target", "release", "app.exe");
const OUT = path.join(
  REPO,
  "docs",
  "evidence",
  "nextgen",
  "ui-layout-audit.json",
);
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

async function getJson(url) {
  const response = await fetch(url);
  if (!response.ok) throw new Error(`HTTP ${response.status}`);
  return response.json();
}

async function pageTargets() {
  for (let attempt = 0; attempt < 80; attempt += 1) {
    try {
      const targets = await getJson(`http://127.0.0.1:${PORT}/json`);
      const pages = targets.filter(
        (target) =>
          target.type === "page" &&
          (target.url.includes("tauri") || (target.title ?? "").includes("SH")),
      );
      if (pages.length) return pages;
    } catch {
      // not ready
    }
    await sleep(500);
  }
  throw new Error("未找到 WebView2 page target");
}

async function evaluateOnSocket(socket, expression, timeoutMs = 12000) {
  return new Promise((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error("CDP evaluate timeout")), timeoutMs);
    socket.addEventListener(
      "message",
      (event) => {
        const message = JSON.parse(event.data);
        if (message.id !== 1) return;
        clearTimeout(timer);
        if (message.error) reject(new Error(JSON.stringify(message.error)));
        else if (message.result?.exceptionDetails) {
          reject(
            new Error(
              `evaluate exception: ${JSON.stringify(
                message.result.exceptionDetails,
              ).slice(0, 200)}`,
            ),
          );
        } else resolve(message.result?.result?.value);
      },
    );
    socket.send(
      JSON.stringify({
        id: 1,
        method: "Runtime.evaluate",
        params: { expression, returnByValue: true },
      }),
    );
  });
}

async function connectTarget(target) {
  const socket = new WebSocket(target.webSocketDebuggerUrl);
  await new Promise((resolve, reject) => {
    socket.addEventListener("open", resolve, { once: true });
    socket.addEventListener("error", reject, { once: true });
  });
  return socket;
}

const PROBE_JS = `({ w: window.innerWidth, h: window.innerHeight, title: document.title })`;

const AUDIT_JS = `(() => {
  const selectors = [
    "button", "input", "select", "textarea", ".nav-item", "[role='tab']",
    ".ui3-page-header", ".ui3-section-head", ".home-hero", ".catalog-card",
    ".download-task-row", ".settings-row", ".dialog", ".recent-row",
    ".quick-action-tile", ".library-card", ".pack-preview",
    ".boot-problems-card", ".distribution-note", ".ui3-page h1",
    ".ui3-page h2", ".global-progress"
  ];
  const elements = [];
  const nodes = [];
  const seen = new Set();
  const overlaySelectors = [".dialog-backdrop", ".toast-stack", ".global-progress", ".download-detail-modal", ".update-modal-backdrop", ".error-modal-backdrop"];
  const isOverlay = (el) => {
    for (const sel of overlaySelectors) if (el.closest(sel)) return true;
    const style = getComputedStyle(el);
    return style.position === "fixed" || style.position === "absolute";
  };
  for (const selector of selectors) {
    for (const el of document.querySelectorAll(selector)) {
      if (seen.has(el)) continue;
      seen.add(el);
      const rect = el.getBoundingClientRect();
      const style = getComputedStyle(el);
      if (style.display === "none" || style.visibility === "hidden" || rect.width <= 0 || rect.height <= 0) continue;
      const text = (el.innerText || "").trim().replace(/\\s+/g, " ").slice(0, 48);
      elements.push({
        selector, tag: el.tagName.toLowerCase(), text,
        className: (typeof el.className === "string" ? el.className : "").slice(0, 80),
        rect: { x: Math.round(rect.x), y: Math.round(rect.y), width: Math.round(rect.width), height: Math.round(rect.height), right: Math.round(rect.right), bottom: Math.round(rect.bottom) },
        scrollWidth: el.scrollWidth, clientWidth: el.clientWidth,
        scrollHeight: el.scrollHeight, clientHeight: el.clientHeight,
        overflowX: style.overflowX, overflowY: style.overflowY,
        fontSize: style.fontSize, lineHeight: style.lineHeight,
        position: style.position, overlay: isOverlay(el)
      });
      nodes.push(el);
    }
  }
  const overlaps = [];
  for (let i = 0; i < elements.length; i += 1) {
    for (let j = i + 1; j < elements.length; j += 1) {
      const a = elements[i], b = elements[j];
      if (a.overlay || b.overlay) continue;
      if (nodes[i].contains(nodes[j]) || nodes[j].contains(nodes[i])) continue;
      const ar = a.rect, br = b.rect;
      const w = Math.min(ar.right, br.right) - Math.max(ar.x, br.x);
      const h = Math.min(ar.bottom, br.bottom) - Math.max(ar.y, br.y);
      if (w <= 0 || h <= 0) continue;
      const area = w * h;
      const min = Math.min(ar.width * ar.height, br.width * br.height);
      if (min > 0 && area / min > 0.12) {
        overlaps.push({ a: { text: a.text, cls: a.className }, b: { text: b.text, cls: b.className }, overlapRatio: Math.round((area / min) * 100) / 100 });
      }
    }
  }
  const overflows = [];
  for (const el of elements) {
    if (el.overflowX === "visible" && el.scrollWidth > el.clientWidth + 2) {
      overflows.push({ text: el.text, cls: el.className, scrollWidth: el.scrollWidth, clientWidth: el.clientWidth, overflowX: el.overflowX });
    }
  }
  const shortWraps = [];
  const walker = document.createTreeWalker(document.body, NodeFilter.SHOW_TEXT);
  let textNode;
  while ((textNode = walker.nextNode())) {
    const text = (textNode.textContent || "").trim();
    if (text.length < 2 || text.length > 8) continue;
    const range = document.createRange();
    range.selectNodeContents(textNode);
    const rects = range.getClientRects();
    if (rects.length <= 1) continue;
    const tops = Array.from(rects).map((rect) => rect.top);
    const spread = Math.max(...tops) - Math.min(...tops);
    if (spread > 1) {
      shortWraps.push({
        text,
        cls: (textNode.parentElement?.className || "").slice(0, 80),
        lines: rects.length,
        spread: Math.round(spread),
      });
    }
  }
  const root = document.documentElement;
  return {
    viewport: { width: root.clientWidth, height: root.clientHeight },
    horizontalScrolls: root.scrollWidth > root.clientWidth + 2 ? [{ scrollWidth: root.scrollWidth, clientWidth: root.clientWidth }] : [],
    elements, overlaps, overflows, singleCharacterWraps: shortWraps
  };
})()`;

const CLICK_JS = (label) => `(() => {
  const targets = [...document.querySelectorAll("button")];
  const hit = targets.find((el) => (el.innerText || "").trim().includes(${JSON.stringify(label)}));
  if (!hit) return false;
  hit.click();
  return true;
})()`;

async function main() {
  try {
    execFileSync("taskkill", ["/IM", "app.exe", "/F"], { stdio: "ignore" });
  } catch {
    // not running
  }
  await sleep(800);
  const child = spawn(EXE, [], {
    detached: true,
    stdio: "ignore",
    env: {
      ...process.env,
      WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${PORT}`,
    },
  });
  child.unref();

  let mainTarget = null;
  let socket = null;
  for (let attempt = 0; attempt < 60; attempt += 1) {
    try {
      const targets = await pageTargets();
      for (const target of targets) {
        const candidate = await connectTarget(target);
        const probe = await evaluateOnSocket(candidate, PROBE_JS, 4000);
        if (probe?.w >= 1000) {
          mainTarget = target;
          socket = candidate;
          break;
        }
        candidate.close();
      }
      if (socket) break;
    } catch {
      // retry
    }
    await sleep(800);
  }
  if (!socket) throw new Error("主窗口 target 不可用");
  await sleep(3500);

  const pages = [];
  const labels = ["home", "library", "discover", "downloads", "accounts", "settings"];
  const navLabels = ["首页", "游戏库", "发现", "下载", "账户", "设置"];
  for (let index = 0; index < labels.length; index += 1) {
    if (index > 0) {
      await evaluateOnSocket(socket, CLICK_JS(navLabels[index]), 6000);
      await sleep(1100);
    }
    const audit = await evaluateOnSocket(socket, AUDIT_JS, 12000);
    pages.push({ name: labels[index], ...audit });
    console.log(
      `[ui-layout-audit] ${labels[index]}: elements=${audit.elements.length} overlaps=${audit.overlaps.length} overflows=${audit.overflows.length} wraps=${audit.singleCharacterWraps.length}`,
    );
  }
  socket.close();

  const result = {
    runtime: "tauri",
    source: "getBoundingClientRect",
    devHead: "4c908c80e9d3dd0b7ff6601d72d82624fa1f8cd8",
    generatedAt: new Date().toISOString(),
    pages,
  };
  fs.mkdirSync(path.dirname(OUT), { recursive: true });
  fs.writeFileSync(OUT, JSON.stringify(result, null, 2) + "\n");
  console.log(`[ui-layout-audit] written ${OUT}`);
}

main()
  .then(() => process.exit(0))
  .catch((error) => {
    console.error(`[ui-layout-audit] FAIL: ${error.message}`);
    process.exit(1);
  });
