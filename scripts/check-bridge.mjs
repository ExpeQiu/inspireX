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

function httpSpec(cmd, args) {
  return {
    method: "POST",
    path: "/v1/invoke",
    body: JSON.stringify({ cmd, args: args ?? {} }),
  };
}

const spec = httpSpec("create_tag", { name: "红", color: "#FF5F57" });
if (spec.path !== "/v1/invoke" || spec.method !== "POST") {
  console.error("FAIL: 命令应走 /v1/invoke", spec);
  process.exit(1);
}
const body = JSON.parse(spec.body);
if (body.cmd !== "create_tag" || body.args.name !== "红") {
  console.error("FAIL: invoke 体", body);
  process.exit(1);
}

function browseThumbPath(filePath, maxSide = 240) {
  return `/v1/browse/thumb?path=${encodeURIComponent(filePath)}&maxSide=${maxSide}`;
}
function ingestMediaUrl(origin, port, pathAndQuery) {
  if (origin && isLocalHttpOrigin(origin)) return pathAndQuery;
  return `http://127.0.0.1:${port}${pathAndQuery}`;
}
const thumb = browseThumbPath("/Volumes/Q-V-P/a.jpg", 240);
if (!thumb.startsWith("/v1/browse/thumb?") || !thumb.includes("maxSide=240")) {
  console.error("FAIL: thumb path", thumb);
  process.exit(1);
}
const localThumb = ingestMediaUrl("http://localhost:1420", "18765", thumb);
if (localThumb !== thumb) {
  console.error("FAIL: Vite 页缩略图应 same-origin", localThumb);
  process.exit(1);
}
const remoteThumb = ingestMediaUrl("vscode-webview://x", "18765", thumb);
if (!remoteThumb.startsWith("http://127.0.0.1:18765/v1/browse/thumb")) {
  console.error("FAIL: webview 缩略图应走 ingest", remoteThumb);
  process.exit(1);
}

console.log("OK: bridge fallback rules");
