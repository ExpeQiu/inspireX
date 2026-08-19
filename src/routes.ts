/** 浏览器主界面子页面路径（localhost:1420/…）。 */

export type Mode = "browse" | "library" | "subscribe" | "generate";
export type BrowseKind = "volumes" | "local" | "hub";

export type RouteState = {
  mode: Mode;
  browseKind: BrowseKind;
  viewFolderId: string | null;
  browsePath: string | null;
  tagFilter: string | null;
  colorFilter: string | null;
};

const VIRTUAL_PREFIX = "inspirex://";

export const PATHS = {
  library: "/library",
  volumes: "/volumes",
  local: "/local",
  hub: "/hub",
  subscribe: "/subscribe",
  generate: "/generate",
} as const;

export function isVirtualBrowsePath(path: string | null | undefined): boolean {
  return !path || path.startsWith(VIRTUAL_PREFIX);
}

export function folderPath(id: string): string {
  return `/folders/${encodeURIComponent(id)}`;
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
  if (segs === PATHS.generate) {
    return { ...base, mode: "generate", browsePath: null };
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
    (a.colorFilter || "") === (b.colorFilter || "")
  );
}

export function defaultBrowsePath(kind: BrowseKind, roots: { volumes: string; local: string; hub: string }): string | null {
  if (kind === "local") return roots.local || virtualRootOf("local");
  if (kind === "hub") return roots.hub || null;
  return roots.volumes || virtualRootOf("volumes");
}
