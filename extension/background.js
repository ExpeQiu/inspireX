importScripts("pinterest.js");

const PX = globalThis.__inspirexPinterest;
const DEFAULTS = {
  port: 18765,
  token: "inspirex-local",
};
const MAX_WATCH_IMPORT = 20;
const MAX_WATCH_COLLECT = 80;
const SEEN_CAP = 4000;

function ensureMenus() {
  chrome.contextMenus.removeAll(() => {
    chrome.contextMenus.create({
      id: "inspirex-save-image",
      title: "保存到灵犀 InspireX",
      contexts: ["image"],
    });
    chrome.contextMenus.create({
      id: "inspirex-save-page-images",
      title: "本页多选保存到灵犀",
      contexts: ["page", "image"],
    });
    chrome.contextMenus.create({
      id: "inspirex-watch-page",
      title: "盯梢此页（定时入库）",
      contexts: ["page"],
    });
  });
}

function ensureSyncAlarm() {
  chrome.alarms.create("inspirex-watch-sync", { delayInMinutes: 1, periodInMinutes: 2 });
}

chrome.runtime.onInstalled.addListener(() => {
  ensureMenus();
  ensureSyncAlarm();
  restoreWatches().catch((e) => console.warn("[inspirex] restore", e));
});
chrome.runtime.onStartup.addListener(() => {
  ensureMenus();
  ensureSyncAlarm();
  restoreWatches().catch((e) => console.warn("[inspirex] restore", e));
});

chrome.contextMenus.onClicked.addListener(async (info, tab) => {
  if (info.menuItemId === "inspirex-save-page-images") {
    if (tab && tab.id) await openPicker(tab.id);
    return;
  }
  if (info.menuItemId === "inspirex-watch-page") {
    try {
      const r = await toggleWatch(tab);
      console.log("[inspirex] watch", r);
    } catch (e) {
      console.error("[inspirex] watch", e);
    }
    return;
  }
  if (info.menuItemId !== "inspirex-save-image") return;
  const imageUrl = info.srcUrl;
  if (!imageUrl) return;
  try {
    const result = await saveToInspireX({
      imageUrl,
      pageUrl: info.pageUrl || (tab && tab.url) || "",
      title: (tab && tab.title) || "",
    });
    console.log("[inspirex]", result);
  } catch (e) {
    console.error("[inspirex]", e);
  }
});

async function getConfig() {
  const stored = await chrome.storage.sync.get(DEFAULTS);
  return Object.assign({}, DEFAULTS, stored);
}

async function assertIngestUp(port) {
  const ctrl = new AbortController();
  const t = setTimeout(() => ctrl.abort(), 2500);
  try {
    const res = await fetch("http://127.0.0.1:" + port + "/health", { signal: ctrl.signal });
    if (!res.ok) throw new Error("HTTP " + res.status);
  } catch (e) {
    throw new Error("灵犀采集未就绪，请先 ./scripts/start.sh");
  } finally {
    clearTimeout(t);
  }
}

function bytesToBase64(buf) {
  const bytes = new Uint8Array(buf);
  let bin = "";
  const chunk = 0x8000;
  for (let i = 0; i < bytes.length; i += chunk) {
    bin += String.fromCharCode.apply(null, bytes.subarray(i, i + chunk));
  }
  return btoa(bin);
}

function sleep(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

async function fetchOneImage(url, pageUrl) {
  const ctrl = new AbortController();
  const t = setTimeout(() => ctrl.abort(), 12000);
  try {
    const res = await fetch(url, {
      signal: ctrl.signal,
      headers: {
        Accept: "image/avif,image/webp,image/*,*/*;q=0.8",
        Referer: pageUrl || "https://www.pinterest.com/",
      },
    });
    if (!res.ok) throw new Error("图片 HTTP " + res.status);
    const buf = await res.arrayBuffer();
    if (!buf.byteLength) throw new Error("图片为空");
    return {
      base64: bytesToBase64(buf),
      mime: (res.headers.get("content-type") || "image/jpeg").split(";")[0],
    };
  } finally {
    clearTimeout(t);
  }
}

async function fetchImageInChrome(url, pageUrl) {
  const candidates = PX.pinimgCandidates ? PX.pinimgCandidates(url) : [url];
  let lastErr = new Error("无候选图片");
  for (let i = 0; i < candidates.length; i++) {
    try {
      return await fetchOneImage(candidates[i], pageUrl);
    } catch (e) {
      lastErr = e;
    }
  }
  throw lastErr;
}

async function saveToInspireX(payload) {
  const cfg = await getConfig();
  await assertIngestUp(cfg.port);
  let inline = null;
  try {
    inline = await fetchImageInChrome(payload.imageUrl, payload.pageUrl);
  } catch (e) {
    console.warn("[inspirex] chrome_fetch_fail", e);
    throw new Error("浏览器下载图片失败: " + (e && e.message ? e.message : e));
  }
  const body = {
    imageUrl: payload.imageUrl,
    pageUrl: payload.pageUrl || null,
    title: payload.title || null,
    notes: payload.notes || null,
    imageBase64: inline.base64,
    mime: inline.mime,
    sourceRef: payload.sourceRef || null,
  };
  if (payload.folderId) body.folderId = payload.folderId;
  const res = await fetch("http://127.0.0.1:" + cfg.port + "/v1/import", {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
      Authorization: "Bearer " + cfg.token,
    },
    body: JSON.stringify(body),
  });
  const data = await res.json();
  if (!res.ok || !data.ok) {
    throw new Error(data.error || "HTTP " + res.status);
  }
  return data;
}

async function saveBatchToInspireX(payload) {
  const cfg = await getConfig();
  const urls = (payload.imageUrls || []).slice(0, 30);
  const res = await fetch("http://127.0.0.1:" + cfg.port + "/v1/import/batch", {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
      Authorization: "Bearer " + cfg.token,
    },
    body: JSON.stringify({
      imageUrls: urls,
      pageUrl: payload.pageUrl || null,
      title: payload.title || null,
      notes: payload.notes || null,
    }),
  });
  if (res.status === 404) {
    let imported = 0;
    let skipped = 0;
    const errors = [];
    for (const imageUrl of urls) {
      try {
        const r = await saveToInspireX({
          imageUrl,
          pageUrl: payload.pageUrl,
          title: payload.title,
        });
        imported += r.imported || 0;
        skipped += r.skipped || 0;
      } catch (e) {
        errors.push(String(e));
      }
    }
    return { ok: errors.length < urls.length || imported > 0, imported, skipped, errors };
  }
  const data = await res.json();
  if (!res.ok) {
    throw new Error(data.error || "HTTP " + res.status);
  }
  return data;
}

async function openPicker(tabId) {
  try {
    await chrome.scripting.executeScript({
      target: { tabId },
      files: ["picker.js"],
    });
  } catch (e) {
    console.error("[inspirex] picker", e);
    throw e;
  }
}

chrome.runtime.onMessage.addListener((msg, _sender, sendResponse) => {
  if (msg && msg.type === "inspirex-save") {
    saveToInspireX(msg.payload)
      .then((r) => sendResponse({ ok: true, result: r }))
      .catch((e) => sendResponse({ ok: false, error: String(e) }));
    return true;
  }
  if (msg && msg.type === "inspirex-save-batch") {
    saveBatchToInspireX(msg.payload)
      .then((r) =>
        sendResponse({
          ok: true,
          imported: r.imported || 0,
          skipped: r.skipped || 0,
          errors: r.errors || [],
          runId: r.runId,
        })
      )
      .catch((e) => sendResponse({ ok: false, error: String(e) }));
    return true;
  }
  if (msg && msg.type === "inspirex-open-picker") {
    openPicker(msg.tabId)
      .then(() => sendResponse({ ok: true }))
      .catch((e) => sendResponse({ ok: false, error: String(e) }));
    return true;
  }
  if (msg && msg.type === "inspirex-watch-status") {
    watchStatus(msg.tabUrl)
      .then((r) => sendResponse({ ok: true, ...r }))
      .catch((e) => sendResponse({ ok: false, error: String(e) }));
    return true;
  }
  if (msg && msg.type === "inspirex-watch-restore") {
    restoreWatches()
      .then(() => sendResponse({ ok: true }))
      .catch((e) => sendResponse({ ok: false, error: String(e) }));
    return true;
  }
  if (msg && msg.type === "inspirex-watch-toggle") {
    toggleWatch(msg.tab)
      .then((r) => sendResponse({ ok: true, ...r }))
      .catch((e) => sendResponse({ ok: false, error: String(e) }));
    return true;
  }
});

function alarmName(id) {
  return "watch:" + id;
}

function seenKey(id) {
  return "watchSeen:" + id;
}

function normalizePath(url) {
  try {
    const u = new URL(url);
    return u.origin + decodeURIComponent(u.pathname).replace(/\/$/, "");
  } catch {
    return (url || "").split("?")[0].replace(/\/$/, "");
  }
}

function urlsMatch(watchUrl, tabUrl) {
  const a = normalizePath(watchUrl);
  const b = normalizePath(tabUrl);
  return b === a || b.startsWith(a + "/");
}

async function invokeDesktop(cmd, args) {
  const cfg = await getConfig();
  await assertIngestUp(cfg.port);
  const res = await fetch("http://127.0.0.1:" + cfg.port + "/v1/invoke", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ cmd, args: args || {} }),
  });
  const data = await res.json().catch(() => ({}));
  if (!res.ok) throw new Error(data.error || "HTTP " + res.status);
  return data;
}

function collectPageImageUrls() {
  const MIN = 80;
  const seen = {};
  const out = [];
  function absUrl(src) {
    try {
      return new URL(src, location.href).href;
    } catch {
      return src;
    }
  }
  function upgradePinimg(url) {
    try {
      const u = new URL(url, location.href);
      if (!/(^|\.)pinimg\.com$/i.test(u.hostname)) return url;
      u.pathname = u.pathname.replace(/\/\d+x\//, "/originals/");
      return u.href;
    } catch {
      return url;
    }
  }
  function add(src) {
    if (!src || src.indexOf("data:") === 0 || src.indexOf("blob:") === 0) return;
    const url = upgradePinimg(absUrl(src));
    const lower = url.split("?")[0].toLowerCase();
    if (lower.slice(-4) === ".svg" || lower.slice(-4) === ".ico" || lower.indexOf("favicon") >= 0) {
      return;
    }
    if (seen[url]) return;
    seen[url] = 1;
    out.push(url);
  }
  const imgs = document.querySelectorAll("img");
  for (let i = 0; i < imgs.length; i++) {
    const img = imgs[i];
    const src =
      img.currentSrc ||
      img.src ||
      img.getAttribute("data-src") ||
      img.getAttribute("data-lazy-src") ||
      img.getAttribute("data-original");
    const w = img.naturalWidth || img.width || 0;
    const h = img.naturalHeight || img.height || 0;
    const pin = src && /pinimg\.com/i.test(src);
    if (w && h && (w < MIN || h < MIN) && !pin) continue;
    add(src);
    const srcset = img.getAttribute("srcset") || img.getAttribute("data-srcset") || "";
    srcset.split(",").forEach(function (part) {
      const u = part.trim().split(/\s+/)[0];
      if (u) add(u);
    });
  }
  return out.slice(0, 30);
}

async function listWatchSources() {
  const list = await invokeDesktop("list_sources", {});
  return (Array.isArray(list) ? list : []).filter((s) => s.kind === "plugin_watch");
}

async function findWatch(tabUrl) {
  const list = await listWatchSources();
  return list.find((s) => urlsMatch(s.url, tabUrl)) || null;
}

function watchIntervalMin(src) {
  const n = Number(src && src.intervalMin);
  if (Number.isFinite(n) && n >= 1) return Math.max(1, n);
  if (src && PX.isPinterestUrl(src.url)) return 15;
  return 60;
}

async function ensureAlarm(src) {
  const minutes = watchIntervalMin(src);
  await chrome.alarms.create(alarmName(src.id), { periodInMinutes: minutes });
}

async function restoreWatches() {
  ensureSyncAlarm();
  const list = await listWatchSources();
  const alarms = await chrome.alarms.getAll();
  for (const a of alarms) {
    if (a.name.indexOf("watch:") === 0) await chrome.alarms.clear(a.name);
  }
  for (const s of list) {
    if (s.enabled) await ensureAlarm(s);
  }
}

async function watchStatus(tabUrl) {
  if (!tabUrl) return { watching: false };
  const src = await findWatch(tabUrl);
  return {
    watching: !!(src && src.enabled),
    source: src,
    pinterest: PX.isPinterestUrl(tabUrl),
    intervalMin: src ? watchIntervalMin(src) : PX.isPinterestUrl(tabUrl) ? 15 : 60,
  };
}

async function toggleWatch(tab) {
  if (!tab || !tab.url) throw new Error("没有网页地址");
  if (tab.url.indexOf("http") !== 0) throw new Error("系统页面无法盯梢");
  const existing = await findWatch(tab.url);
  if (existing && existing.enabled) {
    await invokeDesktop("set_source_enabled", { sourceId: existing.id, enabled: false });
    await chrome.alarms.clear(alarmName(existing.id));
    await invokeDesktop("report_source_run", {
      sourceId: existing.id,
      summary: "已暂停盯梢",
    });
    return { watching: false, source: existing };
  }
  let src = existing;
  if (!src) {
    src = await invokeDesktop("create_source", {
      req: {
        name: (tab.title || "盯梢").slice(0, 48),
        kind: "plugin_watch",
        url: tab.url.split("?")[0],
        intervalMin: PX.isPinterestUrl(tab.url) ? 15 : 60,
      },
    });
  } else {
    await invokeDesktop("set_source_enabled", { sourceId: src.id, enabled: true });
  }
  await ensureAlarm(src);
  runWatch(src.id).catch((e) => console.warn("[inspirex] watch_now", e));
  return { watching: true, source: src };
}

function isWatchDue(src) {
  const last = src.lastRunAt || src.last_run_at;
  if (!last) return true;
  const ts = Date.parse(last);
  if (!Number.isFinite(ts)) return true;
  const intervalMs = watchIntervalMin(src) * 60 * 1000;
  return Date.now() - ts >= intervalMs;
}

async function syncAndRunDue() {
  const list = await listWatchSources();
  const enabled = list.filter((s) => s.enabled);
  for (const s of enabled) await ensureAlarm(s);
  const due = enabled.filter((s) => isWatchDue(s) && !watchBusy[s.id]);
  if (!due.length) return;
  due.sort((a, b) => {
    const ta = Date.parse(a.lastRunAt || a.last_run_at || "") || 0;
    const tb = Date.parse(b.lastRunAt || b.last_run_at || "") || 0;
    return ta - tb;
  });
  await runWatch(due[0].id);
}

async function loadSeen(sourceId) {
  const key = seenKey(sourceId);
  const data = await chrome.storage.local.get(key);
  const raw = data[key] || {};
  return {
    ids: Array.isArray(raw.ids) ? raw.ids.slice() : [],
    urls: Array.isArray(raw.urls) ? raw.urls.slice() : [],
  };
}

async function saveSeen(sourceId, seen) {
  await chrome.storage.local.set({
    [seenKey(sourceId)]: {
      ids: (seen.ids || []).slice(-SEEN_CAP),
      urls: (seen.urls || []).slice(-SEEN_CAP),
    },
  });
}

function markSeen(seen, pin) {
  if (pin.pinId && seen.ids.indexOf(pin.pinId) < 0) seen.ids.push(pin.pinId);
  if (pin.imageUrl && seen.urls.indexOf(pin.imageUrl) < 0) seen.urls.push(pin.imageUrl);
}

function alreadySeen(seen, pin) {
  if (pin.pinId && seen.ids.indexOf(pin.pinId) >= 0) return true;
  if (pin.imageUrl && seen.urls.indexOf(pin.imageUrl) >= 0) return true;
  return false;
}

function waitTabComplete(tabId, timeoutMs) {
  return new Promise((resolve) => {
    let done = false;
    const finish = (tab) => {
      if (done) return;
      done = true;
      clearTimeout(timer);
      chrome.tabs.onUpdated.removeListener(onUp);
      resolve(tab || null);
    };
    const timer = setTimeout(() => finish(null), timeoutMs);
    function onUp(id, info, tab) {
      if (id !== tabId) return;
      if (info.status === "complete") finish(tab);
    }
    chrome.tabs.onUpdated.addListener(onUp);
    chrome.tabs
      .get(tabId)
      .then((tab) => {
        if (tab && tab.status === "complete") finish(tab);
      })
      .catch(() => {});
  });
}

async function ensureWatchTab(src) {
  const tabs = await chrome.tabs.query({ url: ["https://*/*", "http://*/*"] });
  let tab = tabs.find((t) => t.url && urlsMatch(src.url, t.url));
  if (tab) return { tab, opened: false };
  if (!PX.isPinterestUrl(src.url)) return { tab: null, opened: false };
  tab = await chrome.tabs.create({ url: src.url, active: false, pinned: true });
  const loaded = await waitTabComplete(tab.id, 25000);
  if (loaded && loaded.url) tab = loaded;
  else {
    try {
      tab = await chrome.tabs.get(tab.id);
    } catch (_) {}
  }
  await sleep(2500);
  console.log("[inspirex] watch_tab_open source_id=" + src.id + " tab_id=" + tab.id);
  return { tab, opened: true };
}

async function collectFromTab(tab) {
  if (PX.isPinterestUrl(tab.url)) {
    await chrome.scripting.executeScript({
      target: { tabId: tab.id },
      files: ["pinterest.js"],
    });
    const canScroll = !tab.active;
    const injected = await chrome.scripting.executeScript({
      target: { tabId: tab.id },
      func: async (shouldScroll, limit) => {
        const api = globalThis.__inspirexPinterest;
        if (!api) return { pins: [], scrolled: false };
        if (shouldScroll) await api.scrollBoard(6, 750);
        return { pins: api.collectPins(limit), scrolled: shouldScroll };
      },
      args: [canScroll, MAX_WATCH_COLLECT],
    });
    const result = (injected && injected[0] && injected[0].result) || { pins: [] };
    return Array.isArray(result.pins) ? result.pins : [];
  }
  const injected = await chrome.scripting.executeScript({
    target: { tabId: tab.id },
    func: collectPageImageUrls,
  });
  const urls = (injected && injected[0] && injected[0].result) || [];
  return urls.map((imageUrl) => ({
    imageUrl,
    pinId: "",
    pinUrl: tab.url || "",
  }));
}

function looksLoggedOut(tab) {
  const url = (tab && tab.url) || "";
  const title = (tab && tab.title) || "";
  return /\/login/i.test(url) || /log\s*in|登录|sign in/i.test(title);
}

const watchBusy = {};

async function runWatch(sourceId) {
  if (watchBusy[sourceId]) return;
  watchBusy[sourceId] = true;
  try {
    const list = await listWatchSources();
    const src = list.find((s) => s.id === sourceId);
    if (!src) {
      await chrome.alarms.clear(alarmName(sourceId));
      return;
    }
    if (!src.enabled) {
      await chrome.alarms.clear(alarmName(sourceId));
      return;
    }
    console.log(
      "[inspirex] watch_start source_id=" + src.id + " url=" + src.url + " kind=" + src.kind
    );
    const ensured = await ensureWatchTab(src);
    const tab = ensured.tab;
    if (!tab || !tab.id) {
      await invokeDesktop("report_source_run", {
        sourceId,
        summary: "标签未打开，已跳过（Pinterest 会自动打开看板；其他站点请保持该页打开）",
      });
      return;
    }
    if (looksLoggedOut(tab)) {
      await invokeDesktop("report_source_run", {
        sourceId,
        summary: "看板未登录。请在自动打开的标签里登录 Pinterest，并保持标签不要关。",
      });
      return;
    }
    const pins = await collectFromTab(tab);
    const seen = await loadSeen(src.id);
    const fresh = pins.filter((p) => p.imageUrl && !alreadySeen(seen, p)).slice(0, MAX_WATCH_IMPORT);
    let imported = 0;
    let skipped = 0;
    let failed = 0;
    const folderId = src.folderId || src.folder_id || null;
    for (const pin of fresh) {
      try {
        const r = await saveToInspireX({
          imageUrl: pin.imageUrl,
          pageUrl: pin.pinUrl || tab.url,
          title: tab.title || src.name,
          sourceRef: src.id,
          folderId,
        });
        imported += r.imported || 0;
        skipped += r.skipped || 0;
        markSeen(seen, pin);
      } catch (e) {
        failed += 1;
        console.warn("[inspirex] watch_save", e);
      }
    }
    await saveSeen(src.id, seen);
    const openedHint = ensured.opened ? "已自动打开看板" : "复用已开标签";
    const summary =
      "自动盯梢 · 入库 " +
      imported +
      " · 跳过 " +
      skipped +
      " · 失败 " +
      failed +
      " · 发现 " +
      pins.length +
      " 张 · 本轮新图 " +
      fresh.length +
      "（" +
      openedHint +
      "）";
    await invokeDesktop("report_source_run", { sourceId, summary });
    console.log(
      "[inspirex] watch_done source_id=" +
        src.id +
        " imported=" +
        imported +
        " skipped=" +
        skipped +
        " failed=" +
        failed +
        " collected=" +
        pins.length
    );
  } catch (e) {
    try {
      await invokeDesktop("report_source_run", {
        sourceId,
        summary: String(e && e.message ? e.message : e),
      });
    } catch (_) {}
    console.warn("[inspirex] watch_fail source_id=" + sourceId, e);
  } finally {
    watchBusy[sourceId] = false;
  }
}

chrome.alarms.onAlarm.addListener((alarm) => {
  if (!alarm.name) return;
  if (alarm.name === "inspirex-watch-sync") {
    syncAndRunDue().catch((e) => console.warn("[inspirex] sync", e));
    return;
  }
  if (alarm.name.indexOf("watch:") !== 0) return;
  runWatch(alarm.name.slice("watch:".length)).catch((e) => console.warn("[inspirex] alarm", e));
});

ensureSyncAlarm();
