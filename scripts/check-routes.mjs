#!/usr/bin/env node
/** 与 src/routes.ts 保持一致：子页面路径解析 / 序列化 */

const VIRTUAL_PREFIX = "inspirex://";
const PATHS = {
  library: "/library",
  volumes: "/volumes",
  local: "/local",
  hub: "/hub",
  subscribe: "/subscribe",
  created: "/created",
  generate: "/generate",
  generatePrompt: "/generate/prompt",
  generateCreated: "/generate/created",
  settings: "/settings",
};

function appModule(mode) {
  if (mode === "generate") return "generate";
  if (mode === "settings") return "settings";
  return "library";
}

function isVirtualBrowsePath(path) {
  return !path || path.startsWith(VIRTUAL_PREFIX);
}

function parseAppHref(href) {
  const q = href.indexOf("?");
  const pathname = (q < 0 ? href : href.slice(0, q)) || "/";
  const search = q < 0 ? "" : href.slice(q);
  return parseAppUrl(pathname, search);
}

function folderPath(id) {
  return `/folders/${encodeURIComponent(id)}`;
}

function promptCategoryPath(id) {
  return `${PATHS.generatePrompt}/${encodeURIComponent(id)}`;
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
  if (state.mode === "settings") return PATHS.settings;
  if (state.viewFolderId) {
    return `${folderPath(state.viewFolderId)}${queryOf({ ...state, mode: "library" })}`;
  }
  if (state.mode === "library") return `${PATHS.library}${queryOf(state)}`;
  if (state.mode === "subscribe") return `${PATHS.subscribe}${queryOf(state)}`;
  if (state.mode === "created") return `${PATHS.created}${queryOf(state)}`;
  if (state.mode === "generate") {
    if (state.generateView === "prompt") {
      if (state.promptCategoryId) {
        return `${promptCategoryPath(state.promptCategoryId)}${queryOf(state)}`;
      }
      return `${PATHS.generatePrompt}${queryOf(state)}`;
    }
    if (state.generateView === "created") return `${PATHS.generateCreated}${queryOf(state)}`;
    return `${PATHS.generate}${queryOf(state)}`;
  }
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
    generateView: "generate",
    promptCategoryId: null,
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
  if (segs === PATHS.created) return { ...base, mode: "created", browsePath: null };
  const promptCatMatch = segs.match(/^\/generate\/prompt\/([^/]+)$/);
  if (promptCatMatch) {
    return {
      ...base,
      mode: "generate",
      generateView: "prompt",
      promptCategoryId: decodeURIComponent(promptCatMatch[1]),
      browsePath: null,
    };
  }
  if (segs === PATHS.generatePrompt) {
    return { ...base, mode: "generate", generateView: "prompt", browsePath: null };
  }
  if (segs === PATHS.generateCreated) {
    return { ...base, mode: "generate", generateView: "created", browsePath: null };
  }
  if (segs === PATHS.generate) return { ...base, mode: "generate", generateView: "generate", browsePath: null };
  if (segs === PATHS.settings) return { ...base, mode: "settings", browsePath: null };
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
    (a.colorFilter || "") === (b.colorFilter || "") &&
    (a.mode !== "generate" || (a.generateView || "generate") === (b.generateView || "generate")) &&
    (a.promptCategoryId || "") === (b.promptCategoryId || "")
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
  generateView: "generate",
  promptCategoryId: null,
};

const cases = [
  [{ ...empty, mode: "library" }, "/library"],
  [{ ...empty, mode: "browse", browseKind: "volumes" }, "/volumes"],
  [{ ...empty, mode: "browse", browseKind: "local" }, "/local"],
  [{ ...empty, mode: "browse", browseKind: "hub" }, "/hub"],
  [{ ...empty, mode: "subscribe" }, "/subscribe"],
  [{ ...empty, mode: "created" }, "/created"],
  [{ ...empty, mode: "generate", generateView: "generate" }, "/generate"],
  [{ ...empty, mode: "generate", generateView: "prompt" }, "/generate/prompt"],
  [
    { ...empty, mode: "generate", generateView: "prompt", promptCategoryId: "cat1" },
    "/generate/prompt/cat1",
  ],
  [{ ...empty, mode: "generate", generateView: "created" }, "/generate/created"],
  [{ ...empty, mode: "settings" }, "/settings"],
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

if (appModule("generate") !== "generate") fail("generate 应属 AI 生图模块");
if (appModule("settings") !== "settings") fail("settings 应属设置");
for (const m of ["browse", "library", "subscribe", "created"]) {
  if (appModule(m) !== "library") fail(`${m} 应属素材管理`);
}
if (parseAppHref("/library?tag=%E7%BA%A2").tagFilter !== "红") {
  fail("parseAppHref 应解析 query");
}

console.log("OK: app subpage routes");
