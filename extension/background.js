const DEFAULTS = {
  port: 18765,
  token: "inspirex-local",
};

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
  });
}

chrome.runtime.onInstalled.addListener(ensureMenus);
chrome.runtime.onStartup.addListener(ensureMenus);

chrome.contextMenus.onClicked.addListener(async (info, tab) => {
  if (info.menuItemId === "inspirex-save-page-images") {
    if (tab && tab.id) await openPicker(tab.id);
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

async function fetchImageInChrome(url, pageUrl) {
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
  const res = await fetch("http://127.0.0.1:" + cfg.port + "/v1/import", {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
      Authorization: "Bearer " + cfg.token,
    },
    body: JSON.stringify({
      imageUrl: payload.imageUrl,
      pageUrl: payload.pageUrl || null,
      title: payload.title || null,
      notes: payload.notes || null,
      imageBase64: inline.base64,
      mime: inline.mime,
    }),
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
});
