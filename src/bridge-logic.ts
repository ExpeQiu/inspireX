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

export function httpSpec(cmd: string, args?: object): HttpSpec {
  const rec = (args ?? {}) as Record<string, unknown>;
  const q = new URLSearchParams();
  const str = (k: string) => {
    const v = rec[k];
    if (v === undefined || v === null) return;
    q.set(k, String(v));
  };
  switch (cmd) {
    case "get_browse_root":
      str("kind");
      return {
        method: "GET",
        path: q.toString() ? `/v1/browse/root?${q.toString()}` : "/v1/browse/root",
      };
    case "list_local_roots":
      return { method: "GET", path: "/v1/browse/local-roots" };
    case "add_local_root":
      return {
        method: "POST",
        path: "/v1/browse/local-roots",
        body: JSON.stringify({ path: rec.path ?? "" }),
      };
    case "remove_local_root":
      return {
        method: "POST",
        path: "/v1/browse/local-roots/remove",
        body: JSON.stringify({ path: rec.path ?? "" }),
      };
    case "list_browse_dir":
      str("path");
      str("limit");
      str("offset");
      str("kind");
      return { method: "GET", path: `/v1/browse/list?${q.toString()}` };
    case "list_browse_by_tag":
      str("path");
      str("tag");
      str("limit");
      str("offset");
      str("kind");
      return { method: "GET", path: `/v1/browse/by-tag?${q.toString()}` };
    case "get_library_info":
      return { method: "GET", path: "/v1/library/info" };
    case "get_image_thumb":
      str("path");
      if (rec.maxSide !== undefined) q.set("maxSide", String(rec.maxSide));
      return { method: "GET", path: `/v1/browse/thumb?${q.toString()}` };
    case "list_folders":
      return { method: "GET", path: "/v1/folders" };
    case "list_tags":
      return { method: "GET", path: "/v1/tags" };
    case "create_folder":
      return {
        method: "POST",
        path: "/v1/folders",
        body: JSON.stringify({
          name: rec.name ?? "",
          parentId: rec.parentId ?? rec.parent_id ?? null,
        }),
      };
    case "import_files":
      return {
        method: "POST",
        path: "/v1/import/paths",
        body: JSON.stringify({
          paths: rec.paths ?? [],
          folderId: rec.folderId ?? rec.folder_id ?? null,
        }),
      };
    default:
      throw new Error(`命令 ${cmd} 需要在灵犀窗口中运行`);
  }
}
