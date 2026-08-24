const status = document.getElementById("status");

document.getElementById("health").onclick = async () => {
  const { port } = await chrome.storage.sync.get({ port: 18765 });
  try {
    const res = await fetch(`http://127.0.0.1:${port}/health`);
    const data = await res.json();
    status.textContent = data.ok ? `服务正常 · :${port}` : "异常响应";
  } catch (e) {
    status.textContent = `连不上 :${port}，请先 ./scripts/start.sh`;
  }
};

document.getElementById("opts").onclick = () => {
  chrome.runtime.openOptionsPage();
};

document.getElementById("pick").onclick = async () => {
  const [tab] = await chrome.tabs.query({ active: true, currentWindow: true });
  if (!tab || !tab.id) {
    status.textContent = "没有活动标签";
    return;
  }
  if (tab.url && (tab.url.startsWith("chrome://") || tab.url.startsWith("chrome-extension://"))) {
    status.textContent = "系统页面无法扫描，请打开普通网页";
    return;
  }
  chrome.runtime.sendMessage({ type: "inspirex-open-picker", tabId: tab.id }, (resp) => {
    if (!resp || !resp.ok) {
      status.textContent = (resp && resp.error) || "无法注入选择器，请刷新页面后重试";
      return;
    }
    window.close();
  });
};

const watchBtn = document.getElementById("watch");
const watchHint = document.getElementById("watchHint");

async function refreshWatchUi() {
  chrome.runtime.sendMessage({ type: "inspirex-watch-restore" });
  const [tab] = await chrome.tabs.query({ active: true, currentWindow: true });
  if (!tab || !tab.url || !tab.url.startsWith("http")) {
    watchBtn.disabled = true;
    watchHint.textContent = "请在普通网页上开启盯梢。";
    return;
  }
  chrome.runtime.sendMessage({ type: "inspirex-watch-status", tabUrl: tab.url }, (resp) => {
    if (!resp || !resp.ok) {
      watchHint.textContent = (resp && resp.error) || "无法读取盯梢状态（灵犀需先启动）";
      return;
    }
    watchBtn.textContent = resp.watching ? "停止盯梢" : "盯梢此页";
    const last = resp.source && (resp.source.lastError || resp.source.last_error);
    const when = resp.source && (resp.source.lastRunAt || resp.source.last_run_at);
    const mins = resp.intervalMin || 60;
    const pinterest = resp.pinterest;
    watchHint.textContent = resp.watching
      ? (pinterest
          ? "Pinterest 自动盯梢 · 每 " + mins + " 分钟（可后台打开看板）"
          : "每 " + mins + " 分钟扫描本页（需保持标签打开）") +
        (last ? " · 上次 " + last : "") +
        (when ? " · " + new Date(when).toLocaleString() : "")
      : pinterest
        ? "盯梢后自动打开看板并入库新 Pin；请先在 Chrome 登录 Pinterest。"
        : "盯梢后每 " + mins + " 分钟自动入库新图；关标签则跳过该次。";
  });
}

watchBtn.onclick = async () => {
  const [tab] = await chrome.tabs.query({ active: true, currentWindow: true });
  chrome.runtime.sendMessage({ type: "inspirex-watch-toggle", tab }, (resp) => {
    if (!resp || !resp.ok) {
      status.textContent = (resp && resp.error) || "盯梢失败，请先启动灵犀";
      return;
    }
    status.textContent = resp.watching ? "已开始盯梢，正在扫描本页…" : "已停止盯梢";
    refreshWatchUi();
  });
};

refreshWatchUi();
