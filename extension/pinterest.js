/**
 * Pinterest 看板收集（页面世界 + 插件后台共用）。
 * 不直连站点；只解析用户已打开（或插件代开）的标签 DOM / __PWS_DATA__。
 */
(function (root) {
  const MAX_VISIT = 8000;
  const MAX_DEPTH = 14;

  function isPinterestUrl(url) {
    if (!url) return false;
    try {
      const host = new URL(url).hostname.replace(/^www\./i, "").toLowerCase();
      return host === "pinterest.com" || host.endsWith(".pinterest.com") || host === "pin.it";
    } catch {
      const l = String(url).toLowerCase();
      return l.indexOf("pinterest.com") >= 0 || l.indexOf("pin.it") >= 0;
    }
  }

  function upgradePinimg(url) {
    try {
      const u = new URL(url, typeof location !== "undefined" ? location.href : undefined);
      if (!/(^|\.)pinimg\.com$/i.test(u.hostname)) return url;
      u.pathname = u.pathname.replace(/\/\d+x\//, "/originals/");
      return u.href;
    } catch {
      return url;
    }
  }

  function pinimgCandidates(url) {
    const upgraded = upgradePinimg(url);
    const list = [];
    function push(u) {
      if (u && list.indexOf(u) < 0) list.push(u);
    }
    push(upgraded);
    try {
      const u = new URL(upgraded);
      if (/(^|\.)pinimg\.com$/i.test(u.hostname) && /\/originals\//.test(u.pathname)) {
        push(upgraded.replace(/\/originals\//, "/736x/"));
      }
    } catch (_) {}
    push(url);
    return list;
  }

  function isJunkPinimg(url) {
    const u = String(url || "");
    if (!/pinimg\.com/i.test(u)) return true;
    if (/\/\d+x\d+_RS\//.test(u)) return true;
    if (/\/(30x30|60x60|75x75)(\/|_)/.test(u)) return true;
    if (/default_user|\/avatars\//i.test(u)) return true;
    if (/\.(svg|ico)(\?|$)/i.test(u) || /favicon/i.test(u)) return true;
    return false;
  }

  function pinUrlFromId(id) {
    if (!id || !/^\d+$/.test(String(id))) return "";
    return "https://www.pinterest.com/pin/" + id + "/";
  }

  function extractPinsFromData(data) {
    const out = [];
    const seen = {};
    let visited = 0;

    function add(imageUrl, pinId) {
      if (!imageUrl || !/pinimg\.com/i.test(imageUrl)) return;
      if (isJunkPinimg(imageUrl)) return;
      const id = pinId && /^\d+$/.test(String(pinId)) ? String(pinId) : "";
      if (!id) return;
      const url = upgradePinimg(imageUrl);
      if (seen[id]) return;
      seen[id] = 1;
      out.push({
        imageUrl: url,
        pinId: id,
        pinUrl: pinUrlFromId(id),
      });
    }

    function walk(node, depth) {
      if (!node || depth > MAX_DEPTH || visited > MAX_VISIT || out.length >= 80) return;
      visited += 1;
      if (Array.isArray(node)) {
        for (let i = 0; i < node.length; i++) walk(node[i], depth + 1);
        return;
      }
      if (typeof node !== "object") return;
      const id = node.id || node.pinId || node.pin_id;
      let imageUrl = null;
      const images = node.images || (node.media && node.media.images);
      if (images && typeof images === "object") {
        imageUrl =
          (images.orig && images.orig.url) ||
          (images.originals && images.originals.url) ||
          (images["736x"] && images["736x"].url) ||
          (images["474x"] && images["474x"].url) ||
          (images["236x"] && images["236x"].url);
      }
      if (!imageUrl && typeof node.url === "string" && /pinimg\.com/.test(node.url)) {
        imageUrl = node.url;
      }
      if (imageUrl) add(imageUrl, id);
      const keys = Object.keys(node);
      for (let i = 0; i < keys.length; i++) {
        const k = keys[i];
        if (k === "trackingParams" || k === "clientTrackingParams") continue;
        walk(node[k], depth + 1);
      }
    }

    walk(data, 0);
    return out;
  }

  function collectPinsFromDom() {
    const out = [];
    const seen = {};
    function add(imageUrl, pinId, pinUrl) {
      if (!imageUrl || imageUrl.indexOf("data:") === 0 || imageUrl.indexOf("blob:") === 0) return;
      if (isJunkPinimg(imageUrl)) return;
      const url = upgradePinimg(imageUrl);
      const id = pinId && /^\d+$/.test(String(pinId)) ? String(pinId) : "";
      const key = id || url;
      if (seen[key]) return;
      seen[key] = 1;
      out.push({
        imageUrl: url,
        pinId: id,
        pinUrl: pinUrl || pinUrlFromId(id),
      });
    }

    if (typeof document === "undefined") return out;

    const links = document.querySelectorAll('a[href*="/pin/"]');
    for (let i = 0; i < links.length; i++) {
      const a = links[i];
      const href = a.href || a.getAttribute("href") || "";
      const m = href.match(/\/pin\/(\d+)/);
      const img = a.querySelector("img");
      const src =
        (img && (img.currentSrc || img.src)) ||
        (img && (img.getAttribute("data-src") || img.getAttribute("src")));
      if (m) add(src || "", m[1], href.split("?")[0]);
    }

    const imgs = document.querySelectorAll("img");
    for (let i = 0; i < imgs.length; i++) {
      const img = imgs[i];
      if (img.closest && img.closest('[data-test-id="gestalt-avatar-svg"], [data-test-id="user-rep-avatar"]')) {
        continue;
      }
      const src =
        img.currentSrc ||
        img.src ||
        img.getAttribute("data-src") ||
        img.getAttribute("data-lazy-src") ||
        img.getAttribute("data-original");
      if (src && /pinimg\.com/i.test(src)) add(src, "", "");
    }
    return out;
  }

  function collectPins(limit) {
    const cap = Math.max(1, Number(limit) || 50);
    const merged = [];
    const seen = {};
    function merge(list) {
      for (let i = 0; i < list.length; i++) {
        const p = list[i];
        const key = p.pinId || p.imageUrl;
        if (!key || seen[key]) continue;
        seen[key] = 1;
        merged.push(p);
        if (merged.length >= cap) return;
      }
    }
    if (typeof document !== "undefined") {
      const el = document.getElementById("__PWS_DATA__");
      if (el && el.textContent) {
        try {
          merge(extractPinsFromData(JSON.parse(el.textContent)));
        } catch (_) {}
      }
    }
    merge(collectPinsFromDom());
    return merged.slice(0, cap);
  }

  function scrollBoard(steps, waitMs) {
    steps = Math.max(1, Number(steps) || 6);
    waitMs = Math.max(200, Number(waitMs) || 700);
    return new Promise(function (resolve) {
      if (typeof window === "undefined") {
        resolve({ steps: 0, height: 0 });
        return;
      }
      let i = 0;
      function step() {
        if (i >= steps) {
          resolve({
            steps: i,
            height: document.documentElement.scrollHeight || 0,
          });
          return;
        }
        const dy = Math.max(400, Math.round((window.innerHeight || 800) * 0.85));
        window.scrollBy(0, dy);
        i += 1;
        setTimeout(step, waitMs);
      }
      step();
    });
  }

  const api = {
    isPinterestUrl,
    isJunkPinimg,
    upgradePinimg,
    pinimgCandidates,
    pinUrlFromId,
    extractPinsFromData,
    collectPinsFromDom,
    collectPins,
    scrollBoard,
  };
  root.__inspirexPinterest = api;
  if (typeof module !== "undefined" && module.exports) {
    module.exports = api;
  }
})(typeof globalThis !== "undefined" ? globalThis : this);
