const portEl = document.getElementById("port");
const tokenEl = document.getElementById("token");
const msg = document.getElementById("msg");

chrome.storage.sync.get({ port: 18765, token: "inspirex-local" }).then((v) => {
  portEl.value = v.port;
  tokenEl.value = v.token;
});

document.getElementById("save").onclick = async () => {
  await chrome.storage.sync.set({
    port: Number(portEl.value) || 18765,
    token: tokenEl.value || "inspirex-local",
  });
  msg.textContent = "已保存";
};
