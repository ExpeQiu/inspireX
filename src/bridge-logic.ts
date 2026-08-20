/** 纯逻辑：浏览器预览 / Cursor webview 下桌面桥回退。 */

export type HttpSpec = {
  method: "GET" | "POST";
  path: string;
  body?: string;
};

export function isIpcMissingMessage(msg: string): boolean {
  return (
    msg.includes("reading 'invoke'") ||
    msg.includes('reading "invoke"') ||
    msg.includes("reading 'convertFileSrc'") ||
    msg.includes("__TAURI_INTERNALS__") ||
    msg.includes("is not a function")
  );
}

export function isLocalHttpOrigin(origin: string): boolean {
  return (
    origin.startsWith("http://localhost:") ||
    origin.startsWith("http://127.0.0.1:") ||
    origin === "http://localhost" ||
    origin === "http://127.0.0.1"
  );
}

/** same-origin 仅在 Vite/本机页有效；Cursor webview 等非 localhost origin 会 Failed to fetch。 */
export function ingestBasesFor(origin: string | undefined, port: string): string[] {
  const abs = [`http://127.0.0.1:${port}`, `http://localhost:${port}`];
  if (origin && isLocalHttpOrigin(origin)) {
    return ["", ...abs];
  }
  return abs;
}

export function browseThumbPath(filePath: string, maxSide = 240): string {
  return `/v1/browse/thumb?path=${encodeURIComponent(filePath)}&maxSide=${maxSide}`;
}

export function ingestMediaUrl(
  origin: string | undefined,
  port: string,
  pathAndQuery: string,
): string {
  if (origin && isLocalHttpOrigin(origin)) return pathAndQuery;
  return `http://127.0.0.1:${port}${pathAndQuery}`;
}

/** 全部命令走本机 `/v1/invoke`；插件采集仍用独立 REST。 */
export function httpSpec(cmd: string, args?: object): HttpSpec {
  return {
    method: "POST",
    path: "/v1/invoke",
    body: JSON.stringify({ cmd, args: args ?? {} }),
  };
}
