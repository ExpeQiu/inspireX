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

  function add(src, w, h) {
    if (!src || src.startsWith("data:") || src.startsWith("blob:")) return;
    const url = absUrl(src);
    const lower = url.split("?")[0].toLowerCase();
    if (lower.endsWith(".svg") || lower.endsWith(".ico") || lower.includes("favicon")) return;
    if (seen.has(url)) return;
    seen.add(url);
    items.push({ url, w: w || 0, h: h || 0 });
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
    if (w && h && (w < MIN || h < MIN)) return;
    add(src, w, h);
  });

  const host = document.createElement("div");
  host.id = "inspirex-picker-host";
  host.style.cssText = "all:initial;position:fixed;inset:0;z-index:2147483647;";
  const shadow = host.attachShadow({ mode: "open" });
  document.documentElement.appendChild(host);

  const selected = new Set(items.slice(0, Math.min(items.length, MAX_SAVE)).map((i) => i.url));
  const shown = items.slice(0, MAX_SHOW);

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
      .card img { width:100%; height:110px; object-fit:cover; display:block; background:#ece7dc; }
      .card .meta { font-size:11px; padding:6px 8px; color:#6b655c; white-space:nowrap; overflow:hidden; text-overflow:ellipsis; }
      .card .tick { position:absolute; top:6px; right:6px; width:22px; height:22px; border-radius:6px; background:#2f5d50; color:#fff; display:none; align-items:center; justify-content:center; font-size:13px; }
      .card.on .tick { display:flex; }
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

  function toggle(url) {
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

  function render() {
    renderCount();
    if (!shown.length) {
      grid.innerHTML = `<div class="empty">这一页没有足够大的图片。可对单张右键「保存到灵犀」。</div>`;
      return;
    }
    grid.innerHTML = shown
      .map(
        (it) => `<div class="card ${selected.has(it.url) ? "on" : ""}" data-url="${encodeURIComponent(it.url)}">
          <span class="tick">✓</span>
          <img src="${it.url}" alt="">
          <div class="meta">${it.w && it.h ? `${it.w}×${it.h}` : "图片"}</div>
        </div>`
      )
      .join("");
    grid.querySelectorAll(".card").forEach((card) => {
      card.addEventListener("click", () => toggle(decodeURIComponent(card.getAttribute("data-url"))));
    });
  }

  shadow.querySelector("[data-act=close]").onclick = close;
  shadow.querySelector("[data-act=all]").onclick = () => {
    selected.clear();
    shown.slice(0, MAX_SAVE).forEach((it) => selected.add(it.url));
    render();
  };
  shadow.querySelector("[data-act=none]").onclick = () => {
    selected.clear();
    render();
  };
  shadow.querySelector("[data-act=save]").onclick = () => {
    const urls = Array.from(selected);
    if (!urls.length) {
      statusEl.textContent = "请先勾选图片";
      return;
    }
    statusEl.textContent = `正在保存 ${urls.length} 张…`;
    chrome.runtime.sendMessage(
      {
        type: "inspirex-save-batch",
        payload: {
          imageUrls: urls,
          pageUrl: location.href,
          title: document.title,
        },
      },
      (resp) => {
        if (!resp || !resp.ok) {
          statusEl.textContent = (resp && resp.error) || "保存失败，请确认已启动灵犀窗口";
          return;
        }
        statusEl.textContent = `完成 · 入库 ${resp.imported} · 跳过 ${resp.skipped}` +
          (resp.errors && resp.errors.length ? ` · 错误 ${resp.errors.length}` : "");
        setTimeout(close, 1200);
      }
    );
  };

  render();
})();
