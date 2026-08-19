#!/usr/bin/env node
/** 与 src/bridge-logic.ts 保持一致：IPC 缺失识别 + HTTP 基址选择 */
function isIpcMissingMessage(msg) {
  return (
    msg.includes("reading 'invoke'") ||
    msg.includes('reading "invoke"') ||
    msg.includes("reading 'convertFileSrc'") ||
    msg.includes("__TAURI_INTERNALS__") ||
    msg.includes("is not a function")
  );
}

function isLocalHttpOrigin(origin) {
  return (
    origin.startsWith("http://localhost:") ||
    origin.startsWith("http://127.0.0.1:") ||
    origin === "http://localhost" ||
    origin === "http://127.0.0.1"
  );
}

function ingestBasesFor(origin, port) {
  const abs = [`http://127.0.0.1:${port}`, `http://localhost:${port}`];
  if (origin && isLocalHttpOrigin(origin)) return ["", ...abs];
  return abs;
}

const invokeErr = "TypeError: Cannot read properties of undefined (reading 'invoke')";
if (!isIpcMissingMessage(invokeErr)) {
  console.error("FAIL: invoke undefined 应识别为 IPC 缺失");
  process.exit(1);
}

const webview = ingestBasesFor("vscode-webview://1e", "18765");
if (webview[0] === "" || !webview[0].includes("127.0.0.1")) {
  console.error("FAIL: 非 localhost origin 不得走 same-origin", webview);
  process.exit(1);
}

const vite = ingestBasesFor("http://localhost:1420", "18765");
if (vite[0] !== "") {
  console.error("FAIL: Vite 页应优先 same-origin", vite);
  process.exit(1);
}

console.log("OK: bridge fallback rules");
