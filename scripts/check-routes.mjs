#!/usr/bin/env node
/** 与 src/routes.ts 保持一致：子页面路径解析 / 序列化 */

const VIRTUAL_PREFIX = "inspirex://";
const PATHS = {
  library: "/library",
  volumes: "/volumes",
  local: "/local",
  hub: "/hub",
  subscribe: "/subscribe",
  generate: "/generate",
};

function isVirtualBrowsePath(path) {
  return !path || path.startsWith(VIRTUAL_PREFIX);
}

function folderPath(id) {
  return `/folders/${encodeURIComponent(id)}`;
}

function queryOf(state) {
  const q = new URLSearchParams();
  if (state.mode === "browse" && state.browsePath && !isVirtualBrowsePath(state.browsePath)) {
    q.set("path", state.browsePath);
  }
  if (state.tagFilter) q.set("tag", state.tagFilter);
  if (state.mode === "library" && state.colorFilter) q.set("color", state.colorFilter);
  const s = q.toString();
  return s ? `?${s}` : "";
}

function toHref(state) {
  if (state.viewFolderId) {
    return `${folderPath(state.viewFolderId)}${queryOf({ ...state, mode: "library" })}`;
  }
  if (state.mode === "library") return `${PATHS.library}${queryOf(state)}`;
  if (state.mode === "subscribe") return `${PATHS.subscribe}${queryOf(state)}`;
  if (state.mode === "generate") return `${PATHS.generate}${queryOf(state)}`;
  if (state.browseKind === "local") return `${PATHS.local}${queryOf(state)}`;
  if (state.browseKind === "hub") return `${PATHS.hub}${queryOf(state)}`;
  return `${PATHS.volumes}${queryOf(state)}`;
}

function parseAppUrl(pathname, search) {
  const q = new URLSearchParams(search.startsWith("?") ? search : search ? `?${search}` : "");
  const tagFilter = q.get("tag");
  const colorFilter = q.get("color");
  const browsePath = q.get("path");
  const segs = pathname.replace(/\/+$/, "") || "/";
  const base = {
    mode: "browse",
    browseKind: "volumes",
    viewFolderId: null,
    browsePath,
    tagFilter,
    colorFilter,
  };
  const folderMatch = segs.match(/^\/folders\/([^/]+)$/);
  if (folderMatch) {
    return {
      ...base,
      mode: "library",
      viewFolderId: decodeURIComponent(folderMatch[1]),
      browsePath: null,
    };
  }
  if (segs === PATHS.library) return { ...base, mode: "library", browsePath: null };
  if (segs === PATHS.subscribe) return { ...base, mode: "subscribe", browsePath: null };
  if (segs === PATHS.generate) return { ...base, mode: "generate", browsePath: null };
  if (segs === PATHS.local) return { ...base, mode: "browse", browseKind: "local" };
  if (segs === PATHS.hub) return { ...base, mode: "browse", browseKind: "hub" };
  return { ...base, mode: "browse", browseKind: "volumes" };
}

function sameRoute(a, b) {
  return (
    a.mode === b.mode &&
    a.browseKind === b.browseKind &&
    a.viewFolderId === b.viewFolderId &&
    (a.browsePath || "") === (b.browsePath || "") &&
    (a.tagFilter || "") === (b.tagFilter || "") &&
    (a.colorFilter || "") === (b.colorFilter || "")
  );
}

function fail(msg) {
  console.error(`FAIL: ${msg}`);
  process.exit(1);
}

const empty = {
  mode: "browse",
  browseKind: "volumes",
  viewFolderId: null,
  browsePath: null,
  tagFilter: null,
  colorFilter: null,
};

const cases = [
  [{ ...empty, mode: "library" }, "/library"],
  [{ ...empty, mode: "browse", browseKind: "volumes" }, "/volumes"],
  [{ ...empty, mode: "browse", browseKind: "local" }, "/local"],
  [{ ...empty, mode: "browse", browseKind: "hub" }, "/hub"],
  [{ ...empty, mode: "subscribe" }, "/subscribe"],
  [{ ...empty, mode: "generate" }, "/generate"],
  [{ ...empty, mode: "library", viewFolderId: "abc" }, "/folders/abc"],
  [
    { ...empty, mode: "browse", browseKind: "local", browsePath: "/Users/me/Pictures" },
    "/local?path=%2FUsers%2Fme%2FPictures",
  ],
  [{ ...empty, mode: "library", tagFilter: "红" }, "/library?tag=%E7%BA%A2"],
];

for (const [state, href] of cases) {
  const got = toHref(state);
  if (got !== href) fail(`toHref ${JSON.stringify(state)} => ${got} 期望 ${href}`);
  const parsed = parseAppUrl(href.split("?")[0], href.includes("?") ? href.slice(href.indexOf("?")) : "");
  if (!sameRoute(parsed, { ...empty, ...state, viewFolderId: state.viewFolderId })) {
    fail(`parse 往返失败 ${href} => ${JSON.stringify(parsed)}`);
  }
}

if (parseAppUrl("/", "").browseKind !== "volumes" || parseAppUrl("/", "").mode !== "browse") {
  fail("根路径 / 应落在外置卷");
}
if (!sameRoute(parseAppUrl("/", ""), parseAppUrl("/volumes", ""))) {
  fail("/ 与 /volumes 应同一页");
}
if (toHref({ ...empty, browsePath: "inspirex://volumes" }) !== "/volumes") {
  fail("虚拟根路径不应写入 ?path=");
}

console.log("OK: app subpage routes");
