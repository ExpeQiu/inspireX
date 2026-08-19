(() => {
  if (window.__inspirexPickerOpen) {
    const existing = document.getElementById("inspirex-picker-host");
    if (existing) existing.remove();
  }
  window.__inspirexPickerOpen = true;

  const MIN = 80;
  const MAX_SHOW = 80;
  const MAX_SAVE = 30;
  const seen = new Set();
  const items = [];

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
      u.pathname = u.pathname.replace(/\/\d+x\//, "/736x/");
      return u.href;
    } catch {
      return url;
    }
  }

  function add(src, w, h) {
    if (!src || src.startsWith("data:") || src.startsWith("blob:")) return;
    const url = upgradePinimg(absUrl(src));
    const lower = url.split("?")[0].toLowerCase();
    if (lower.endsWith(".svg") || lower.endsWith(".ico") || lower.includes("favicon")) return;
    if (seen.has(url)) return;
    seen.add(url);
    items.push({ url, w: w || 0, h: h || 0 });
  }

  function isPinimg(url) {
    try {
      return /(^|\.)pinimg\.com$/i.test(new URL(url, location.href).hostname);
    } catch {
      return false;
    }
  }

  document.querySelectorAll("img").forEach((img) => {
    const src =
      img.currentSrc ||
      img.src ||
      img.getAttribute("data-src") ||
      img.getAttribute("data-lazy-src") ||
      img.getAttribute("data-original");
    const w = img.naturalWidth || img.width || 0;
    const h = img.naturalHeight || img.height || 0;
    if (w && h && (w < MIN || h < MIN) && !isPinimg(src || "")) return;
    add(src, w, h);
    const srcset = img.getAttribute("srcset") || img.getAttribute("data-srcset") || "";
    srcset.split(",").forEach((part) => {
      const u = part.trim().split(/\s+/)[0];
      if (u) add(u, w, h);
    });
  });

  const host = document.createElement("div");
  host.id = "inspirex-picker-host";
  host.setAttribute("data-inspirex", "picker");
  host.style.cssText =
    "all:initial;position:fixed;inset:0;z-index:2147483647;pointer-events:auto;";
  const shadow = host.attachShadow({ mode: "open" });
  (document.body || document.documentElement).appendChild(host);

  const selected = new Set(items.slice(0, Math.min(items.length, MAX_SAVE)).map((i) => i.url));
  const shown = items.slice(0, MAX_SHOW);
  /** @type {Record<string, "idle"|"saving"|"ok"|"skip"|"fail">} */
  const saveState = {};
  let saving = false;

  shadow.innerHTML = `
    <style>
      :host { all: initial; }
      .mask { position:fixed; inset:0; background:rgba(20,18,14,.55); font-family:"Avenir Next","Segoe UI",sans-serif; color:#1c1a17; }
      .panel { position:absolute; inset:24px; background:#f6f1e8; border-radius:16px; display:flex; flex-direction:column; overflow:hidden; box-shadow:0 12px 40px rgba(0,0,0,.25); }
      header { display:flex; align-items:center; justify-content:space-between; padding:14px 18px; border-bottom:1px solid #e4ddd0; }
      header strong { font-size:15px; }
      header span { font-size:12px; color:#6b655c; margin-left:8px; }
      .grid { flex:1; overflow:auto; display:grid; grid-template-columns:repeat(auto-fill,minmax(140px,1fr)); gap:10px; padding:14px; }
      .card { position:relative; border:2px solid transparent; border-radius:10px; background:#fff; cursor:pointer; overflow:hidden; }
      .card.on { border-color:#2f5d50; }
      .card.saving { border-color:#c4a35a; }
      .card.ok { border-color:#2f5d50; }
      .card.skip { border-color:#8a8478; }
      .card.fail { border-color:#b54a3c; }
      .card.busy { cursor:default; }
      .card img { width:100%; height:110px; object-fit:cover; display:block; background:#ece7dc; }
      .card .meta { font-size:11px; padding:6px 8px; color:#6b655c; white-space:nowrap; overflow:hidden; text-overflow:ellipsis; }
      .card .tick { position:absolute; top:6px; right:6px; min-width:22px; height:22px; padding:0 6px; border-radius:6px; background:#2f5d50; color:#fff; display:none; align-items:center; justify-content:center; font-size:11px; font-weight:600; }
      .card.on .tick, .card.saving .tick, .card.ok .tick, .card.skip .tick, .card.fail .tick { display:flex; }
      .card.saving .tick { background:#c4a35a; }
      .card.ok .tick { background:#2f5d50; }
      .card.skip .tick { background:#8a8478; }
      .card.fail .tick { background:#b54a3c; }
      .card .badge { position:absolute; left:6px; bottom:28px; font-size:11px; padding:2px 6px; border-radius:6px; background:rgba(28,26,23,.78); color:#fff; display:none; }
      .card.saving .badge, .card.ok .badge, .card.skip .badge, .card.fail .badge { display:block; }
      footer { display:flex; gap:8px; padding:12px 16px; border-top:1px solid #e4ddd0; }
      button { border:0; border-radius:8px; padding:8px 14px; cursor:pointer; font-size:13px; }
      .primary { background:#2f5d50; color:#f4faf7; }
      .ghost { background:#ece7dc; color:#1c1a17; }
      .status { flex:1; font-size:12px; color:#6b655c; align-self:center; }
      .empty { padding:40px; text-align:center; color:#6b655c; }
    </style>
    <div class="mask">
      <div class="panel">
        <header>
          <div><strong>保存到灵犀</strong><span class="count"></span></div>
          <button class="ghost" type="button" data-act="close">关闭</button>
        </header>
        <div class="grid"></div>
        <footer>
          <button class="ghost" type="button" data-act="all">全选</button>
          <button class="ghost" type="button" data-act="none">清空</button>
          <div class="status"></div>
          <button class="primary" type="button" data-act="save">保存选中</button>
        </footer>
      </div>
    </div>
  `;

  const grid = shadow.querySelector(".grid");
  const countEl = shadow.querySelector(".count");
  const statusEl = shadow.querySelector(".status");

  function close() {
    host.remove();
    window.__inspirexPickerOpen = false;
  }

  function renderCount() {
    countEl.textContent = ` · 本页 ${items.length} 张 · 已选 ${selected.size}（最多 ${MAX_SAVE}）`;
  }

  function stateOf(url) {
    return saveState[url] || "idle";
  }

  function tickText(url) {
    switch (stateOf(url)) {
      case "saving":
        return "…";
      case "ok":
        return "✓";
      case "skip":
        return "○";
      case "fail":
        return "!";
      default:
        return selected.has(url) ? "✓" : "";
    }
  }

  function badgeText(url) {
    switch (stateOf(url)) {
      case "saving":
        return "保存中";
      case "ok":
        return "已入库";
      case "skip":
        return "库中已有";
      case "fail":
        return "失败";
      default:
        return "";
    }
  }

  function cardClass(url) {
    const st = stateOf(url);
    const bits = ["card"];
    if (selected.has(url) || st === "ok" || st === "skip") bits.push("on");
    if (st !== "idle") bits.push(st);
    if (saving) bits.push("busy");
    return bits.join(" ");
  }

  function toggle(url) {
    if (saving) return;
    if (stateOf(url) === "ok" || stateOf(url) === "skip") return;
    if (selected.has(url)) selected.delete(url);
    else {
      if (selected.size >= MAX_SAVE) {
        statusEl.textContent = `一次最多保存 ${MAX_SAVE} 张`;
        return;
      }
      selected.add(url);
    }
    render();
  }

  function sendSave(imageUrl) {
    return new Promise((resolve) => {
      let done = false;
      const t = setTimeout(() => {
        if (done) return;
        done = true;
        resolve({ ok: false, error: "超时：请确认已 ./scripts/start.sh，并重新加载插件" });
      }, 20000);
      chrome.runtime.sendMessage(
        {
          type: "inspirex-save",
          payload: {
            imageUrl,
            pageUrl: location.href,
            title: document.title,
          },
        },
        (resp) => {
          if (done) return;
          done = true;
          clearTimeout(t);
          if (chrome.runtime.lastError) {
            resolve({ ok: false, error: chrome.runtime.lastError.message });
            return;
          }
          resolve(resp || { ok: false, error: "无响应" });
        },
      );
    });
  }

  function render() {
    renderCount();
    if (!shown.length) {
      grid.innerHTML = `<div class="empty">这一页没有足够大的图片。可对单张右键「保存到灵犀」。</div>`;
      return;
    }
    grid.innerHTML = shown
      .map((it) => {
        const urlAttr = encodeURIComponent(it.url);
        return `<div class="${cardClass(it.url)}" data-url="${urlAttr}">
          <span class="tick">${tickText(it.url)}</span>
          <span class="badge">${badgeText(it.url)}</span>
          <img src="${it.url}" alt="">
          <div class="meta">${it.w && it.h ? `${it.w}×${it.h}` : "图片"}</div>
        </div>`;
      })
      .join("");
    grid.querySelectorAll(".card").forEach((card) => {
      card.addEventListener("click", () => toggle(decodeURIComponent(card.getAttribute("data-url"))));
    });
  }

  shadow.querySelector("[data-act=close]").onclick = close;
  shadow.querySelector("[data-act=all]").onclick = () => {
    if (saving) return;
    selected.clear();
    shown.slice(0, MAX_SAVE).forEach((it) => selected.add(it.url));
    render();
  };
  shadow.querySelector("[data-act=none]").onclick = () => {
    if (saving) return;
    selected.clear();
    render();
  };
  const saveBtn = shadow.querySelector("[data-act=save]");
  saveBtn.onclick = async () => {
    const urls = Array.from(selected).filter((u) => stateOf(u) !== "ok" && stateOf(u) !== "skip");
    if (!urls.length) {
      statusEl.textContent = "请先勾选图片";
      return;
    }
    saving = true;
    saveBtn.disabled = true;
    let imported = 0;
    let skipped = 0;
    let failed = 0;
    for (let i = 0; i < urls.length; i++) {
      const url = urls[i];
      saveState[url] = "saving";
      statusEl.textContent = `正在保存 ${i + 1}/${urls.length}…`;
      render();
      const resp = await sendSave(url);
      const result = resp && resp.result ? resp.result : resp;
      if (!resp || !resp.ok) {
        saveState[url] = "fail";
        failed += 1;
      } else if ((result.imported || 0) > 0) {
        saveState[url] = "ok";
        imported += 1;
      } else {
        saveState[url] = "skip";
        skipped += 1;
      }
      render();
    }
    saving = false;
    saveBtn.disabled = false;
    statusEl.textContent = `完成 · 入库 ${imported} · 已有 ${skipped} · 失败 ${failed}`;
    render();
  };

  render();
})();
