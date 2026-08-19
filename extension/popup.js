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
