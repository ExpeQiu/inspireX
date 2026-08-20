/** 浏览器主界面子页面路径（localhost:1420/…）。 */

export type Mode = "browse" | "library" | "subscribe" | "created" | "generate" | "settings";
export type BrowseKind = "volumes" | "local" | "hub";
export type AppModule = "library" | "generate" | "settings";
export type GenerateView = "prompt" | "generate" | "created";

export type RouteState = {
  mode: Mode;
  browseKind: BrowseKind;
  viewFolderId: string | null;
  browsePath: string | null;
  tagFilter: string | null;
  colorFilter: string | null;
  generateView?: GenerateView;
  promptCategoryId?: string | null;
};

const VIRTUAL_PREFIX = "inspirex://";

export const PATHS = {
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
} as const;

/** browse / library / subscribe / created / 文件夹都属素材管理；generate 是 AI 生图；settings 独立。 */
export function appModule(mode: Mode): AppModule {
  if (mode === "generate") return "generate";
  if (mode === "settings") return "settings";
  return "library";
}

export function isVirtualBrowsePath(path: string | null | undefined): boolean {
  return !path || path.startsWith(VIRTUAL_PREFIX);
}

export function parseAppHref(href: string): RouteState {
  const q = href.indexOf("?");
  const pathname = (q < 0 ? href : href.slice(0, q)) || "/";
  const search = q < 0 ? "" : href.slice(q);
  return parseAppUrl(pathname, search);
}

export function folderPath(id: string): string {
  return `/folders/${encodeURIComponent(id)}`;
}

export function promptCategoryPath(id: string): string {
  return `${PATHS.generatePrompt}/${encodeURIComponent(id)}`;
}

function virtualRootOf(kind: BrowseKind): string | null {
  if (kind === "local") return "inspirex://local";
  if (kind === "volumes") return "inspirex://volumes";
  return null;
}

function queryOf(state: RouteState): string {
  const q = new URLSearchParams();
  if (state.mode === "browse" && state.browsePath && !isVirtualBrowsePath(state.browsePath)) {
    q.set("path", state.browsePath);
  }
  if (state.tagFilter) q.set("tag", state.tagFilter);
  if (state.mode === "library" && state.colorFilter) q.set("color", state.colorFilter);
  const s = q.toString();
  return s ? `?${s}` : "";
}

export function toHref(state: RouteState): string {
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

export function parseAppUrl(pathname: string, search: string): RouteState {
  const q = new URLSearchParams(search.startsWith("?") ? search : search ? `?${search}` : "");
  const tagFilter = q.get("tag");
  const colorFilter = q.get("color");
  const browsePath = q.get("path");
  const segs = pathname.replace(/\/+$/, "") || "/";

  const base: RouteState = {
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

  if (segs === PATHS.library) {
    return { ...base, mode: "library", browsePath: null };
  }
  if (segs === PATHS.subscribe) {
    return { ...base, mode: "subscribe", browsePath: null };
  }
  if (segs === PATHS.created) {
    return { ...base, mode: "created", browsePath: null };
  }
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
  if (segs === PATHS.generate) {
    return { ...base, mode: "generate", generateView: "generate", browsePath: null };
  }
  if (segs === PATHS.settings) {
    return { ...base, mode: "settings", browsePath: null };
  }
  if (segs === PATHS.local) {
    return { ...base, mode: "browse", browseKind: "local" };
  }
  if (segs === PATHS.hub) {
    return { ...base, mode: "browse", browseKind: "hub" };
  }
  // `/` 与 `/volumes` 都是外置卷，兼容 start.sh 打开的根地址
  return { ...base, mode: "browse", browseKind: "volumes" };
}

export function sameRoute(a: RouteState, b: RouteState): boolean {
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

export function defaultBrowsePath(kind: BrowseKind, roots: { volumes: string; local: string; hub: string }): string | null {
  if (kind === "local") return roots.local || virtualRootOf("local");
  if (kind === "hub") return roots.hub || null;
  return roots.volumes || virtualRootOf("volumes");
}
