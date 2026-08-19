import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type DragEvent as ReactDragEvent,
  type MouseEvent,
  type PointerEvent as ReactPointerEvent,
  type ReactNode,
} from "react";
import { convertFileSrc, hasTauriIpc, invoke } from "./bridge";
import { pickFiles, splitPathList } from "./desktop";
import { FolderPicker, type FolderPickerKind } from "./FolderPicker";
import {
  defaultBrowsePath,
  folderPath,
  parseAppUrl,
  PATHS,
  sameRoute,
  toHref,
  type BrowseKind,
  type Mode,
} from "./routes";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import { startDrag } from "@crabnebula/tauri-plugin-drag";
import "./App.css";

type FolderPickerState = {
  purpose: "index" | "hub";
  kind: FolderPickerKind;
};

type Asset = {
  id: string;
  filename: string;
  storedPath: string;
  thumbPath?: string | null;
  dominantColor?: string | null;
  clipStatus: string;
  createdAt: string;
  tags: string[];
  width?: number | null;
  height?: number | null;
  sourceUrl?: string | null;
  notes?: string | null;
  folderId?: string | null;
  sourceKind?: string | null;
  sourceRef?: string | null;
};

type LibraryInfo = {
  libraryPath: string;
  aiMockMode: boolean;
  assetCount: number;
  ingestPort?: number;
  clipModel?: string;
  imageGenProvider?: string;
};

type Folder = {
  id: string;
  name: string;
  parentId?: string | null;
  itemCount?: number;
};

type Tag = {
  id: string;
  name: string;
  color?: string | null;
};

type ImportResult = {
  imported: number;
  skipped: number;
  errors: string[];
  runId: string;
};

type Source = {
  id: string;
  name: string;
  kind: string;
  url: string;
  intervalMin: number;
  folderId?: string | null;
  enabled: boolean;
  lastRunAt?: string | null;
  lastError?: string | null;
  createdAt: string;
};

type GenJob = {
  id: string;
  prompt: string;
  n: number;
  size: string;
  provider: string;
  model: string;
  status: string;
  folderId?: string | null;
  error?: string | null;
  runId?: string | null;
  imported: number;
  createdAt: string;
  finishedAt?: string | null;
  outputPaths?: string[];
};

type PickedGen = {
  path: string;
  jobId: string;
};

type ExternalKind = "folder" | "image" | "video";

type ExternalItem = {
  path: string;
  name: string;
  kind: ExternalKind;
  fileSize?: number | null;
  modifiedAt?: number | null;
  mime?: string | null;
};

type ExternalListing = {
  root: string;
  path: string;
  parent?: string | null;
  folders: ExternalItem[];
  media: ExternalItem[];
  truncated: boolean;
  totalMedia: number;
};

type FolderContents = {
  folder: Folder;
  assets: Asset[];
  media: ExternalItem[];
};

type FinderTag = {
  name: string;
  color: number;
  hex: string;
};

type FinderTagValue = {
  name: string;
  color: number;
};

type PinnedFolder = { name: string; path: string };

type HubSuggestion = { name: string; path: string };
type HubStatus = {
  bound: boolean;
  path: string;
  suggestions: HubSuggestion[];
};
type HubCopyResult = {
  copied: number;
  skipped: number;
  dest: string;
  errors: string[];
};

function sourceLabelOf(kind: BrowseKind): string {
  if (kind === "local") return "本地库";
  if (kind === "hub") return "中转站";
  return "外置卷";
}

function browseRootOf(
  kind: BrowseKind,
  volumesRoot: string,
  localRoot: string,
  hubRoot: string,
): string {
  if (kind === "local") return localRoot;
  if (kind === "hub") return hubRoot;
  return volumesRoot;
}

const LOCAL_VIRTUAL = "inspirex://local";
const VOLUMES_VIRTUAL = "inspirex://volumes";

function virtualRootOf(kind: BrowseKind): string {
  if (kind === "local") return LOCAL_VIRTUAL;
  if (kind === "volumes") return VOLUMES_VIRTUAL;
  return "";
}

function isKindVirtualPath(path: string | null | undefined, virtual: string): boolean {
  if (!path) return true;
  return path === virtual;
}

function spaNav(e: MouseEvent<HTMLAnchorElement>, apply: () => void) {
  if (e.metaKey || e.ctrlKey || e.shiftKey || e.altKey || e.button !== 0) return;
  e.preventDefault();
  apply();
}

function boundRootOf(
  browsePath: string | null,
  roots: PinnedFolder[],
  virtual: string,
): string | null {
  if (!browsePath || isKindVirtualPath(browsePath, virtual)) return null;
  const hit = roots.find((r) => {
    const root = r.path.replace(/\/$/, "");
    return browsePath === root || browsePath.startsWith(`${root}/`);
  });
  return hit?.path ?? null;
}

const SIDEBAR_HIDDEN_KEY = "inspirex.sidebarHiddenPaths";
const FINDER_CUSTOM_KEY = "inspirex.finderCustomTags";
const FINDER_HIDDEN_KEY = "inspirex.finderHiddenTags";
const PINNED_FOLDERS_KEY = "inspirex.pinnedFolders";
const BROWSE_PAGE_SIZE = 72;
const THUMB_MAX_CONCURRENCY = 4;
const FINDER_PALETTE: FinderTag[] = [
  { name: "红色", color: 6, hex: "#FF5F57" },
  { name: "橙色", color: 7, hex: "#FEBC2E" },
  { name: "黄色", color: 5, hex: "#F5C518" },
  { name: "绿色", color: 2, hex: "#28C840" },
  { name: "蓝色", color: 4, hex: "#2A9DF4" },
  { name: "紫色", color: 3, hex: "#AF52DE" },
  { name: "灰色", color: 1, hex: "#A3A3A3" },
];

function hexForFinderColor(color: number): string {
  return FINDER_PALETTE.find((t) => t.color === color)?.hex ?? "#C7C1B6";
}

function libraryTagHex(t: Tag): string {
  if (t.color && t.color.startsWith("#")) return t.color;
  const pal = FINDER_PALETTE.find((p) => p.name === t.name);
  if (pal) return pal.hex;
  let h = 0;
  for (const c of t.name) h = (h + c.charCodeAt(0)) % FINDER_PALETTE.length;
  return FINDER_PALETTE[h].hex;
}

function displayFolderName(name: string): string {
  if (/^Photos Library\.photoslibrary$/i.test(name)) return "照片";
  if (name.toLowerCase().endsWith(".photoslibrary")) {
    return name.replace(/\.photoslibrary$/i, "");
  }
  return name;
}

type ThumbJob = {
  id: number;
  run: () => Promise<void>;
};

let thumbJobSeq = 0;
let thumbActive = 0;
const thumbQueue: ThumbJob[] = [];

function pumpThumbQueue() {
  while (thumbActive < THUMB_MAX_CONCURRENCY && thumbQueue.length > 0) {
    const job = thumbQueue.shift()!;
    thumbActive += 1;
    job
      .run()
      .catch(() => undefined)
      .finally(() => {
        thumbActive -= 1;
        pumpThumbQueue();
      });
  }
}

function enqueueThumb(run: () => Promise<void>): () => void {
  const id = ++thumbJobSeq;
  let cancelled = false;
  const job: ThumbJob = {
    id,
    run: async () => {
      if (cancelled) return;
      await run();
    },
  };
  thumbQueue.push(job);
  pumpThumbQueue();
  return () => {
    cancelled = true;
    const idx = thumbQueue.findIndex((j) => j.id === id);
    if (idx >= 0) thumbQueue.splice(idx, 1);
  };
}

function loadSidebarHidden(): Set<string> {
  try {
    const raw = localStorage.getItem(SIDEBAR_HIDDEN_KEY);
    if (!raw) return new Set();
    const arr = JSON.parse(raw) as unknown;
    if (!Array.isArray(arr)) return new Set();
    return new Set(arr.filter((x): x is string => typeof x === "string"));
  } catch {
    return new Set();
  }
}

function saveSidebarHidden(paths: Set<string>) {
  localStorage.setItem(SIDEBAR_HIDDEN_KEY, JSON.stringify([...paths]));
}

function loadFinderCustom(): FinderTag[] {
  try {
    const raw = localStorage.getItem(FINDER_CUSTOM_KEY);
    if (!raw) return [];
    const arr = JSON.parse(raw) as unknown;
    if (!Array.isArray(arr)) return [];
    return arr.filter(
      (x): x is FinderTag =>
        !!x &&
        typeof x === "object" &&
        typeof (x as FinderTag).name === "string" &&
        typeof (x as FinderTag).color === "number",
    ).map((t) => ({ ...t, hex: t.hex || hexForFinderColor(t.color) }));
  } catch {
    return [];
  }
}

function saveFinderCustom(tags: FinderTag[]) {
  localStorage.setItem(FINDER_CUSTOM_KEY, JSON.stringify(tags));
}

function loadFinderHidden(): Set<string> {
  try {
    const raw = localStorage.getItem(FINDER_HIDDEN_KEY);
    if (!raw) return new Set();
    const arr = JSON.parse(raw) as unknown;
    if (!Array.isArray(arr)) return new Set();
    return new Set(arr.filter((x): x is string => typeof x === "string"));
  } catch {
    return new Set();
  }
}

function saveFinderHidden(names: Set<string>) {
  localStorage.setItem(FINDER_HIDDEN_KEY, JSON.stringify([...names]));
}

function loadPinnedFolders(): PinnedFolder[] {
  try {
    const raw = localStorage.getItem(PINNED_FOLDERS_KEY);
    if (!raw) return [];
    const arr = JSON.parse(raw) as unknown;
    if (!Array.isArray(arr)) return [];
    return arr.filter(
      (x): x is PinnedFolder =>
        !!x &&
        typeof x === "object" &&
        typeof (x as PinnedFolder).name === "string" &&
        typeof (x as PinnedFolder).path === "string",
    );
  } catch {
    return [];
  }
}

function savePinnedFolders(items: PinnedFolder[]) {
  localStorage.setItem(PINNED_FOLDERS_KEY, JSON.stringify(items));
}

function folderNameFromPath(path: string): string {
  const parts = path.replace(/\/$/, "").split("/").filter(Boolean);
  return parts[parts.length - 1] || path;
}

function absInLibrary(libraryRoot: string, rel: string): string {
  return `${libraryRoot.replace(/\/$/, "")}/${rel}`;
}

function assetSrc(libraryRoot: string, asset: Asset): string {
  const rel = asset.thumbPath || asset.storedPath;
  return convertFileSrc(absInLibrary(libraryRoot, rel));
}

function fmtSize(n?: number | null): string {
  if (!n || n <= 0) return "—";
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
  return `${(n / 1024 / 1024).toFixed(1)} MB`;
}

function sourceKindLabel(kind?: string | null): string {
  switch (kind) {
    case "web":
      return "订阅";
    case "plugin":
      return "插件";
    case "ai":
      return "AI";
    default:
      return "本地";
  }
}

function jobStatusLabel(status: string): string {
  switch (status) {
    case "running":
      return "生成中";
    case "generated":
      return "已生成";
    case "done":
      return "已入库";
    case "error":
      return "失败";
    default:
      return status;
  }
}

function fileBaseName(path: string): string {
  const parts = path.replace(/\\/g, "/").split("/");
  return parts[parts.length - 1] || path;
}

const aspectCache = new Map<string, string>();

function ratioFromSize(w?: number | null, h?: number | null): string | undefined {
  if (!w || !h || w <= 0 || h <= 0) return undefined;
  return `${w} / ${h}`;
}

function RatioThumb({
  src,
  alt,
  cacheKey,
  width,
  height,
  background,
}: {
  src?: string | null;
  alt: string;
  cacheKey: string;
  width?: number | null;
  height?: number | null;
  background?: string;
}) {
  const known = ratioFromSize(width, height);
  const [ratio, setRatio] = useState(
    () => known ?? aspectCache.get(cacheKey) ?? "3 / 4",
  );
  const [ready, setReady] = useState(() => Boolean(known || aspectCache.get(cacheKey)));

  useEffect(() => {
    if (!known) return;
    aspectCache.set(cacheKey, known);
    setRatio(known);
    setReady(true);
  }, [cacheKey, known]);

  return (
    <div
      className={`thumb${ready ? " thumb-ready" : ""}`}
      style={{
        background: background || "var(--surface-2)",
        aspectRatio: ready ? ratio : undefined,
      }}
    >
      {src ? (
        <img
          src={src}
          alt={alt}
          loading="lazy"
          onLoad={(e) => {
            const r = ratioFromSize(
              e.currentTarget.naturalWidth,
              e.currentTarget.naturalHeight,
            );
            if (r) {
              aspectCache.set(cacheKey, r);
              setRatio(r);
            }
            setReady(true);
          }}
        />
      ) : null}
    </div>
  );
}

function ExternalThumb({
  item,
  large = false,
}: {
  item: ExternalItem;
  large?: boolean;
}) {
  const rootRef = useRef<HTMLDivElement | null>(null);
  const [visible, setVisible] = useState(large);
  const [src, setSrc] = useState<string | null>(null);
  const [err, setErr] = useState(false);
  const [ratio, setRatio] = useState(
    () =>
      aspectCache.get(item.path) ??
      (item.kind === "video" ? "16 / 9" : "3 / 4"),
  );
  const [ready, setReady] = useState(() => aspectCache.has(item.path));

  const rememberRatio = useCallback((w: number, h: number) => {
    const r = ratioFromSize(w, h);
    if (r) {
      aspectCache.set(item.path, r);
      setRatio(r);
    }
    setReady(true);
  }, [item.path]);

  useEffect(() => {
    const cached = aspectCache.get(item.path);
    setRatio(cached ?? (item.kind === "video" ? "16 / 9" : "3 / 4"));
    setReady(Boolean(cached));
  }, [item.path, item.kind]);

  useEffect(() => {
    if (large || visible) return;
    const el = rootRef.current;
    if (!el) return;
    const io = new IntersectionObserver(
      (entries) => {
        if (entries.some((e) => e.isIntersecting)) {
          setVisible(true);
          io.disconnect();
        }
      },
      { root: null, rootMargin: "240px", threshold: 0.01 },
    );
    io.observe(el);
    return () => io.disconnect();
  }, [large, visible]);

  useEffect(() => {
    if (!visible) return;
    let cancelled = false;
    setSrc(null);
    setErr(false);

    if (item.kind === "video") {
      setSrc(convertFileSrc(item.path));
      return;
    }

    const cancelQueue = enqueueThumb(async () => {
      if (cancelled) return;
      try {
        const thumbPath = await invoke<string>("get_image_thumb", {
          path: item.path,
          maxSide: large ? 960 : 240,
        });
        if (!cancelled) setSrc(convertFileSrc(thumbPath));
      } catch {
        if (!cancelled) setSrc(convertFileSrc(item.path));
      }
    });

    return () => {
      cancelled = true;
      cancelQueue();
    };
  }, [item.path, item.kind, large, visible]);

  let body: ReactNode;
  if (err) {
    body = <div className="thumb-fallback">无法预览</div>;
  } else if (!visible || !src) {
    body = <div className="thumb-fallback">{visible ? "加载中…" : ""}</div>;
  } else if (item.kind === "video") {
    body = (
      <div className="thumb-inner">
        <video
          src={src}
          muted
          controls={large}
          preload={large ? "metadata" : "none"}
          className="video-thumb"
          onLoadedMetadata={(e) =>
            rememberRatio(e.currentTarget.videoWidth, e.currentTarget.videoHeight)
          }
          onError={() => setErr(true)}
        />
      </div>
    );
  } else {
    body = (
      <div className="thumb-inner">
        <img
          src={src}
          alt={item.name}
          loading="lazy"
          decoding="async"
          onLoad={(e) =>
            rememberRatio(e.currentTarget.naturalWidth, e.currentTarget.naturalHeight)
          }
          onError={() => setErr(true)}
        />
      </div>
    );
  }

  if (large) {
    return <div ref={rootRef}>{body}</div>;
  }
  return (
    <div
      ref={rootRef}
      className={`thumb${ready ? " thumb-ready" : ""}`}
      style={ready ? { aspectRatio: ratio } : undefined}
    >
      {body}
    </div>
  );
}

export default function App() {
  const bootRef = useRef(parseAppUrl(window.location.pathname, window.location.search));
  const boot = bootRef.current;
  const skipUrlSync = useRef(false);
  const [mode, setMode] = useState<Mode>(boot.mode);
  const [info, setInfo] = useState<LibraryInfo | null>(null);
  const [assets, setAssets] = useState<Asset[]>([]);
  const [folders, setFolders] = useState<Folder[]>([]);
  const [tags, setTags] = useState<Tag[]>([]);
  const [folderId, setFolderId] = useState<string | null>(null);
  const [viewFolderId, setViewFolderId] = useState<string | null>(boot.viewFolderId);
  const [folderAssets, setFolderAssets] = useState<Asset[]>([]);
  const [folderMedia, setFolderMedia] = useState<ExternalItem[]>([]);
  const [libSelected, setLibSelected] = useState<Set<string>>(() => new Set());
  const [tagFilter, setTagFilter] = useState<string | null>(boot.tagFilter);
  const [colorFilter, setColorFilter] = useState<string | null>(boot.colorFilter);
  const [query, setQuery] = useState("");
  const [useClip, setUseClip] = useState(true);
  const [status, setStatus] = useState("");
  const [busy, setBusy] = useState(false);
  const [selected, setSelected] = useState<Asset | null>(null);
  const [tagDraft, setTagDraft] = useState("");
  const [notesDraft, setNotesDraft] = useState("");
  const [sourceDraft, setSourceDraft] = useState("");
  const [browseSelected, setBrowseSelected] = useState<Set<string>>(() => new Set());

  const [sources, setSources] = useState<Source[]>([]);
  const [sourceName, setSourceName] = useState("");
  const [sourceUrl, setSourceUrl] = useState("");
  const [sourceKind, setSourceKind] = useState<"rss" | "url_list" | "html_page">("rss");
  const [sourceInterval, setSourceInterval] = useState(360);
  const [sourceFilterId, setSourceFilterId] = useState<string | null>(null);

  const [genJobs, setGenJobs] = useState<GenJob[]>([]);
  const [genPrompt, setGenPrompt] = useState("");
  const [genN, setGenN] = useState(1);
  const [genSize, setGenSize] = useState("1024x1024");
  const [genFilterId, setGenFilterId] = useState<string | null>(null);
  const [pickedGen, setPickedGen] = useState<PickedGen | null>(null);

  const [browseKind, setBrowseKind] = useState<BrowseKind>(boot.browseKind);
  const [volumesRoot, setVolumesRoot] = useState(VOLUMES_VIRTUAL);
  const [volumeRoots, setVolumeRoots] = useState<PinnedFolder[]>([]);
  const [localRoot, setLocalRoot] = useState(LOCAL_VIRTUAL);
  const [localRoots, setLocalRoots] = useState<PinnedFolder[]>([]);
  const [hubRoot, setHubRoot] = useState("");
  const [hubSuggestions, setHubSuggestions] = useState<HubSuggestion[]>([]);
  const [browsePath, setBrowsePath] = useState<string | null>(boot.browsePath);
  const [listing, setListing] = useState<ExternalListing | null>(null);
  const [browseError, setBrowseError] = useState<string | null>(null);
  const [picked, setPicked] = useState<ExternalItem | null>(null);
  const [pickedTags, setPickedTags] = useState<FinderTagValue[]>([]);
  const [pickedTagDraft, setPickedTagDraft] = useState("");
  const [customFinderTags, setCustomFinderTags] = useState<FinderTag[]>(() => loadFinderCustom());
  const [hiddenFinderTags, setHiddenFinderTags] = useState<Set<string>>(() => loadFinderHidden());
  const [pinnedFolders, setPinnedFolders] = useState<PinnedFolder[]>(() => loadPinnedFolders());
  const [pathAsk, setPathAsk] = useState<null | "files" | "hub-files">(null);
  const [pathDraft, setPathDraft] = useState("");
  const [folderPicker, setFolderPicker] = useState<FolderPickerState | null>(null);
  const [sidebarHidden, setSidebarHidden] = useState<Set<string>>(() => loadSidebarHidden());
  const listingRef = useRef<ExternalListing | null>(null);
  listingRef.current = listing;
  const browseAnchorRef = useRef<string | null>(null);
  const tagFilterRef = useRef<string | null>(null);
  tagFilterRef.current = tagFilter;
  const libDragRef = useRef<{ asset: Asset; x: number; y: number } | null>(null);
  const libDragStarted = useRef(false);

  const libraryRoot = info?.libraryPath ?? "";
  const browseRoot = browseRootOf(browseKind, volumesRoot, localRoot, hubRoot);
  const sourceLabel = sourceLabelOf(browseKind);
  const hubBound = Boolean(hubRoot);
  const detailOpen = Boolean(picked || selected);
  const routeHref = toHref({
    mode,
    browseKind,
    viewFolderId,
    browsePath,
    tagFilter,
    colorFilter,
  });

  useEffect(() => {
    if (skipUrlSync.current) {
      skipUrlSync.current = false;
      return;
    }
    const cur = `${window.location.pathname}${window.location.search}`;
    if (cur === routeHref) return;
    const curRoute = parseAppUrl(window.location.pathname, window.location.search);
    const nextRoute = parseAppUrl(
      routeHref.split("?")[0],
      routeHref.includes("?") ? routeHref.slice(routeHref.indexOf("?")) : "",
    );
    if (sameRoute(curRoute, nextRoute)) {
      history.replaceState(null, "", routeHref);
    } else {
      history.pushState(null, "", routeHref);
    }
    console.info("[inspirex]", { event: "route", href: routeHref });
  }, [routeHref]);

  useEffect(() => {
    if (mode !== "generate") setPickedGen(null);
  }, [mode]);

  useEffect(() => {
    const onPop = () => {
      const next = parseAppUrl(window.location.pathname, window.location.search);
      skipUrlSync.current = true;
      setMode(next.mode);
      setBrowseKind(next.browseKind);
      setViewFolderId(next.viewFolderId);
      setTagFilter(next.tagFilter);
      setColorFilter(next.colorFilter);
      setPicked(null);
      setSelected(null);
      if (next.browsePath) {
        setBrowsePath(next.browsePath);
        return;
      }
      setBrowsePath(
        defaultBrowsePath(next.browseKind, {
          volumes: volumesRoot,
          local: localRoot,
          hub: hubRoot,
        }),
      );
    };
    window.addEventListener("popstate", onPop);
    return () => window.removeEventListener("popstate", onPop);
  }, [volumesRoot, localRoot, hubRoot]);

  const hideFromSidebar = useCallback((path: string, name: string) => {
    setSidebarHidden((prev) => {
      const next = new Set(prev);
      next.add(path);
      saveSidebarHidden(next);
      return next;
    });
    setStatus(`已从侧栏隐藏「${name}」（未删除原文件）`);
  }, []);

  const restoreSidebarHidden = useCallback(() => {
    setSidebarHidden(() => {
      const next = new Set<string>();
      saveSidebarHidden(next);
      return next;
    });
    setHiddenFinderTags(() => {
      const next = new Set<string>();
      saveFinderHidden(next);
      return next;
    });
    setStatus("已恢复侧栏隐藏的目录和标签");
  }, []);

  const hideFinderTag = useCallback((name: string) => {
    setHiddenFinderTags((prev) => {
      const next = new Set(prev);
      next.add(name);
      saveFinderHidden(next);
      return next;
    });
    setTagFilter((cur) => (cur === name ? null : cur));
    setStatus(`已从侧栏移除标签「${name}」（文件上的标签仍保留）`);
  }, []);

  const onCreateTag = useCallback(async () => {
    const name = window.prompt("新标签名称")?.trim();
    if (!name) return;
    if (
      FINDER_PALETTE.some((t) => t.name === name) ||
      customFinderTags.some((t) => t.name === name) ||
      tags.some((t) => t.name === name)
    ) {
      setTagFilter(name);
      return;
    }
    const used = new Set(tags.map((t) => t.color).filter(Boolean));
    const pal = FINDER_PALETTE.find((p) => !used.has(p.hex)) ?? FINDER_PALETTE[6];
    const next: FinderTag = { name, color: pal.color, hex: pal.hex };
    setCustomFinderTags((prev) => {
      const list = [...prev, next];
      saveFinderCustom(list);
      return list;
    });
    try {
      await invoke("create_tag", { name, color: pal.hex });
      const tagList = await invoke<Tag[]>("list_tags");
      setTags(tagList);
      setTagFilter(name);
      setStatus(`已添加标签「${name}」，各来源都可筛选和贴上`);
    } catch (e) {
      setTagFilter(name);
      setStatus(`已添加侧栏标签「${name}」· 入库标签: ${e}`);
    }
  }, [customFinderTags, tags]);

  const cycleFinderTagColor = useCallback((name: string) => {
    const order = [6, 7, 5, 2, 4, 3, 1, 0];
    setCustomFinderTags((prev) => {
      const list = prev.map((t) => {
        if (t.name !== name) return t;
        const i = order.indexOf(t.color);
        const color = order[(i + 1) % order.length];
        return { ...t, color, hex: hexForFinderColor(color) };
      });
      saveFinderCustom(list);
      return list;
    });
  }, []);

  const visibleBrowseFolders = useMemo(
    () => (listing?.folders ?? []).filter((f) => !sidebarHidden.has(f.path)),
    [listing, sidebarHidden],
  );

  const volumeRoot = useMemo(
    () => boundRootOf(browsePath, volumeRoots, volumesRoot || VOLUMES_VIRTUAL),
    [browsePath, volumeRoots, volumesRoot],
  );

  const localBoundRoot = useMemo(
    () => (browseKind === "local" ? boundRootOf(browsePath, localRoots, localRoot || LOCAL_VIRTUAL) : null),
    [browseKind, browsePath, localRoots, localRoot],
  );

  const sourceNavRoot =
    browseKind === "local" ? localBoundRoot : browseKind === "volumes" ? volumeRoot : null;

  const suggestedVisible = useMemo(() => {
    const items: PinnedFolder[] = [];
    if (browseKind === "volumes") {
      for (const r of volumeRoots) {
        if (!sidebarHidden.has(r.path)) items.push(r);
      }
    }
    if (browseKind === "local") {
      for (const r of localRoots) {
        if (!sidebarHidden.has(r.path)) items.push(r);
      }
    }
    for (const p of pinnedFolders) {
      if (sidebarHidden.has(p.path)) continue;
      if (items.some((x) => x.path === p.path)) continue;
      items.push(p);
    }
    return items;
  }, [browseKind, volumeRoots, localRoots, sidebarHidden, pinnedFolders]);

  const visibleFinderTags = useMemo(() => {
    const seen = new Set<string>();
    const list: FinderTag[] = [];
    const fromLib: FinderTag[] = tags.map((t) => ({
      name: t.name,
      color: FINDER_PALETTE.find((p) => p.hex === t.color)?.color ?? 0,
      hex: libraryTagHex(t),
    }));
    for (const t of [...FINDER_PALETTE, ...customFinderTags, ...fromLib]) {
      if (seen.has(t.name) || hiddenFinderTags.has(t.name)) continue;
      seen.add(t.name);
      list.push({ ...t, hex: t.hex || hexForFinderColor(t.color) });
    }
    return list;
  }, [customFinderTags, hiddenFinderTags, tags]);

  const refreshLibrary = useCallback(async () => {
    const [lib, folds, tagList] = await Promise.all([
      invoke<LibraryInfo>("get_library_info"),
      invoke<Folder[]>("list_folders"),
      invoke<Tag[]>("list_tags"),
    ]);
    setInfo(lib);
    setFolders(folds);
    setTags(tagList);

    if (colorFilter) {
      const list = await invoke<Asset[]>("search_assets", {
        query: { colorHex: colorFilter, useClip: false, limit: 200 },
      });
      setAssets(list);
      return;
    }
    if (tagFilter) {
      const list = await invoke<Asset[]>("search_assets", {
        query: { tag: tagFilter, useClip: false, limit: 200 },
      });
      setAssets(list);
      return;
    }
    const list = await invoke<Asset[]>("list_assets", { folderId, limit: 200 });
    setAssets(list);
  }, [folderId, tagFilter, colorFilter]);

  const refreshFolders = useCallback(async () => {
    const folds = await invoke<Folder[]>("list_folders");
    setFolders(folds);
  }, []);

  const refreshFolderView = useCallback(async (id: string) => {
    const contents = await invoke<FolderContents>("list_folder_contents", {
      folderId: id,
    });
    setFolderAssets(contents.assets);
    setFolderMedia(contents.media);
    setFolders((prev) =>
      prev.map((f) =>
        f.id === id
          ? {
              ...f,
              itemCount: contents.assets.length + contents.media.length,
            }
          : f,
      ),
    );
    setStatus(
      `文件夹 · ${contents.folder.name} · ${contents.assets.length + contents.media.length} 项（虚拟索引，不复制文件）`,
    );
  }, []);

  const refreshSources = useCallback(async () => {
    const list = await invoke<Source[]>("list_sources");
    setSources(list);
  }, []);

  const refreshSubscribeResults = useCallback(async (ref?: string | null) => {
    const [lib, folds, tagList] = await Promise.all([
      invoke<LibraryInfo>("get_library_info"),
      invoke<Folder[]>("list_folders"),
      invoke<Tag[]>("list_tags"),
    ]);
    setInfo(lib);
    setFolders(folds);
    setTags(tagList);
    const isFeed = (a: Asset) => a.sourceKind === "web" || a.sourceKind === "plugin";
    let list: Asset[];
    if (tagFilter) {
      const tagged = await invoke<Asset[]>("search_assets", {
        query: { tag: tagFilter, useClip: false, limit: 400 },
      });
      list = tagged.filter((a) => isFeed(a) && (!ref || a.sourceRef === ref));
    } else if (ref) {
      list = await invoke<Asset[]>("list_assets", {
        sourceKind: "web",
        sourceRef: ref,
        limit: 400,
      });
    } else {
      const [web, plugin] = await Promise.all([
        invoke<Asset[]>("list_assets", { sourceKind: "web", limit: 400 }),
        invoke<Asset[]>("list_assets", { sourceKind: "plugin", limit: 400 }),
      ]);
      list = [...web, ...plugin].sort((a, b) =>
        (b.createdAt || "").localeCompare(a.createdAt || ""),
      );
    }
    setAssets(list);
  }, [tagFilter]);

  const refreshGenerateResults = useCallback(async (ref?: string | null) => {
    const [lib, folds, jobs, tagList] = await Promise.all([
      invoke<LibraryInfo>("get_library_info"),
      invoke<Folder[]>("list_folders"),
      invoke<GenJob[]>("list_gen_jobs", { limit: 50 }),
      invoke<Tag[]>("list_tags"),
    ]);
    setInfo(lib);
    setFolders(folds);
    setGenJobs(jobs);
    setTags(tagList);
    let list: Asset[];
    if (tagFilter) {
      const tagged = await invoke<Asset[]>("search_assets", {
        query: { tag: tagFilter, useClip: false, limit: 400 },
      });
      list = tagged.filter(
        (a) => a.sourceKind === "ai" && (!ref || a.sourceRef === ref),
      );
    } else {
      list = await invoke<Asset[]>("list_assets", {
        sourceKind: "ai",
        sourceRef: ref ?? null,
        limit: 400,
      });
    }
    setAssets(list);
  }, [tagFilter]);

  const browseReqId = useRef(0);

  const loadHubStatus = useCallback(async () => {
    const status = await invoke<HubStatus>("get_hub_status");
    setHubRoot(status.path);
    setHubSuggestions(status.suggestions);
    return status;
  }, []);

  const refreshBrowse = useCallback(async (path?: string | null, append = false) => {
    const req = ++browseReqId.current;
    setBusy(true);
    if (!append) setBrowseError(null);
    try {
      if (browseKind === "hub") {
        const status = await loadHubStatus();
        if (req !== browseReqId.current) return;
        if (!status.bound) {
          setListing(null);
          setBrowseError(null);
          setStatus("中转站尚未绑定云同步文件夹");
          return;
        }
      }
      const root = await invoke<string>("get_browse_root", { kind: browseKind });
      if (req !== browseReqId.current) return;
      if (browseKind === "local") setLocalRoot(root);
      else if (browseKind === "hub") setHubRoot(root);
      else setVolumesRoot(root);
      const target = path ?? root;
      if (browseKind === "hub" && !target) {
        setListing(null);
        setStatus("中转站尚未绑定云同步文件夹");
        return;
      }
      const prev = listingRef.current;
      const offset = append && prev && prev.path === target ? prev.media.length : 0;
      if (!append) {
        setPicked(null);
        setBrowseSelected(new Set());
      }
      const tag = tagFilterRef.current;
      const data = tag
        ? await invoke<ExternalListing>("list_browse_by_tag", {
            path: target,
            tag,
            kind: browseKind,
            limit: BROWSE_PAGE_SIZE,
            offset,
          })
        : await invoke<ExternalListing>("list_browse_dir", {
            path: target,
            kind: browseKind,
            limit: BROWSE_PAGE_SIZE,
            offset,
          });
      if (req !== browseReqId.current) return;
      // 不回写 browsePath（canonicalize 路径与请求不一致会触发刷新死循环）
      let shown = data.media.length;
      setListing((cur) => {
        if (append && cur && (cur.path === data.path || cur.path === target)) {
          shown = cur.media.length + data.media.length;
          return {
            ...data,
            path: target,
            folders: cur.folders,
            media: [...cur.media, ...data.media],
            truncated: data.truncated,
          };
        }
        return { ...data, path: target };
      });
      setBrowseError(null);
      console.info("[inspirex]", {
        event: "browse_grid",
        layout: "masonry",
        path: target,
        media: shown,
        folders: data.folders.length,
        append,
      });
      const atIndexVirtual =
        (browseKind === "local" && isKindVirtualPath(target, root || LOCAL_VIRTUAL)) ||
        (browseKind === "volumes" && isKindVirtualPath(target, root || VOLUMES_VIRTUAL));
      setStatus(
        (tag ? `标签 · ${tag} · ` : atIndexVirtual
          ? `${sourceLabel} · ${data.folders.length} 个索引目录`
          : `${sourceLabel} · ${target} · `) +
          (atIndexVirtual && !tag
            ? ""
            : `${data.folders.length} 文件夹 · ${data.totalMedia} 媒体`) +
          (data.truncated || shown < data.totalMedia
            ? `（已显示 ${shown}/${data.totalMedia}）`
            : ""),
      );
    } catch (e) {
      if (req !== browseReqId.current) return;
      const msg = e instanceof Error ? e.message : String(e);
      setStatus(`浏览失败: ${msg}`);
      if (!append) {
        setListing(null);
        setBrowseError(msg);
      }
    } finally {
      if (req === browseReqId.current) setBusy(false);
    }
  }, [browseKind, sourceLabel, loadHubStatus]);

  const applyFinderTagToSelection = useCallback(
    async (tag: FinderTag, remove = false) => {
      if (mode === "browse") {
        const paths =
          browseSelected.size > 0
            ? [...browseSelected]
            : picked
              ? [picked.path]
              : [];
        if (paths.length === 0) {
          setStatus("请先勾选素材，再贴标签（与 Finder 把文件拖到标签上相同）");
          return;
        }
        try {
          const n = await invoke<number>("apply_finder_tag", {
            paths,
            tag: tag.name,
            color: tag.color,
            remove,
          });
          setStatus(
            remove
              ? `已去掉「${tag.name}」· ${n} 个文件`
              : `已贴上「${tag.name}」· ${n} 个文件`,
          );
          if (picked && paths.includes(picked.path)) {
            const next = await invoke<FinderTagValue[]>("get_finder_tags", {
              path: picked.path,
            });
            setPickedTags(next);
          }
          if (tagFilterRef.current) {
            await refreshBrowse(browsePath);
          }
        } catch (e) {
          setStatus(`标签失败: ${e}`);
        }
        return;
      }
      if (!selected) {
        setStatus("请先点开一张素材，再点标签旁 + 贴上");
        return;
      }
      const has = selected.tags.includes(tag.name);
      if (!remove && has) {
        setStatus(`「${selected.filename}」已有标签 ${tag.name}`);
        return;
      }
      const nextTags = remove
        ? selected.tags.filter((n) => n !== tag.name)
        : [...selected.tags, tag.name];
      try {
        await invoke("set_asset_tags", { assetId: selected.id, tags: nextTags });
        setSelected({ ...selected, tags: nextTags });
        setTagDraft(nextTags.join(", "));
        setStatus(remove ? `已去掉「${tag.name}」` : `已贴上「${tag.name}」`);
        if (mode === "library") await refreshLibrary();
        else if (mode === "subscribe") await refreshSubscribeResults(sourceFilterId);
        else if (mode === "generate") await refreshGenerateResults(genFilterId);
      } catch (e) {
        setStatus(`标签失败: ${e}`);
      }
    },
    [
      mode,
      browseSelected,
      picked,
      browsePath,
      refreshBrowse,
      selected,
      refreshLibrary,
      refreshSubscribeResults,
      refreshGenerateResults,
      sourceFilterId,
      genFilterId,
    ],
  );

  const togglePickedTag = useCallback(
    async (tag: FinderTag, remove: boolean) => {
      if (!picked) return;
      try {
        await invoke<number>("apply_finder_tag", {
          paths: [picked.path],
          tag: tag.name,
          color: tag.color,
          remove,
        });
        const next = await invoke<FinderTagValue[]>("get_finder_tags", {
          path: picked.path,
        });
        setPickedTags(next);
        setStatus(remove ? `已去掉「${tag.name}」` : `已贴上「${tag.name}」`);
        console.info("[inspirex]", {
          event: "detail_tag",
          path: picked.path,
          tag: tag.name,
          remove,
        });
        if (tagFilterRef.current) await refreshBrowse(browsePath);
      } catch (e) {
        setStatus(`标签失败: ${e}`);
      }
    },
    [picked, browsePath, refreshBrowse],
  );

  const addPickedTagByName = useCallback(async () => {
    const name = pickedTagDraft.trim();
    if (!name || !picked) return;
    const existing =
      visibleFinderTags.find((t) => t.name === name) ??
      FINDER_PALETTE.find((t) => t.name === name) ??
      customFinderTags.find((t) => t.name === name);
    let tag = existing;
    if (!tag) {
      const used = new Set(tags.map((t) => t.color).filter(Boolean));
      const pal = FINDER_PALETTE.find((p) => !used.has(p.hex)) ?? FINDER_PALETTE[6];
      tag = { name, color: pal.color, hex: pal.hex };
      setCustomFinderTags((prev) => {
        const list = [...prev, tag!];
        saveFinderCustom(list);
        return list;
      });
      try {
        await invoke("create_tag", { name, color: pal.hex });
        setTags(await invoke<Tag[]>("list_tags"));
      } catch {
        /* 侧栏标签仍可用 */
      }
    }
    setPickedTagDraft("");
    await togglePickedTag(tag, false);
  }, [
    pickedTagDraft,
    picked,
    visibleFinderTags,
    customFinderTags,
    tags,
    togglePickedTag,
  ]);

  const loadMoreBrowse = useCallback(() => {
    const cur = listingRef.current;
    if (!browsePath || !cur?.truncated || busy) return;
    refreshBrowse(browsePath, true).catch((e) => setStatus(String(e)));
  }, [browsePath, busy, refreshBrowse]);

  useEffect(() => {
    invoke<Tag[]>("list_tags")
      .then(setTags)
      .catch(() => undefined);
    invoke<Folder[]>("list_folders")
      .then(setFolders)
      .catch(() => undefined);
    invoke<LibraryInfo>("get_library_info")
      .then(setInfo)
      .catch((e) => setStatus(String(e)));
    invoke<string>("get_browse_root", { kind: "volumes" })
      .then((r) => {
        const root = r || VOLUMES_VIRTUAL;
        setVolumesRoot(root);
        setBrowsePath((cur) => {
          if (cur) return cur;
          const landed = bootRef.current;
          if (landed.mode === "browse" && landed.browseKind === "volumes") return root;
          return cur;
        });
      })
      .catch((e) => {
        const msg = e instanceof Error ? e.message : String(e);
        setBrowsePath((cur) => {
          if (cur) return cur;
          const landed = bootRef.current;
          if (landed.mode === "browse" && landed.browseKind === "volumes") return VOLUMES_VIRTUAL;
          return cur;
        });
        setBrowseError(msg);
        setStatus(`浏览根目录失败: ${msg}`);
      });
    invoke<PinnedFolder[]>("list_index_roots", { kind: "volumes" })
      .then(setVolumeRoots)
      .catch(() => undefined);
    invoke<string>("get_browse_root", { kind: "local" })
      .then((r) => {
        const root = r || LOCAL_VIRTUAL;
        setLocalRoot(root);
        setBrowsePath((cur) => {
          if (cur) return cur;
          const landed = bootRef.current;
          if (landed.mode === "browse" && landed.browseKind === "local") return root;
          return cur;
        });
      })
      .catch(() => setLocalRoot(LOCAL_VIRTUAL));
    invoke<PinnedFolder[]>("list_index_roots", { kind: "local" })
      .then(setLocalRoots)
      .catch(() => undefined);
    invoke<HubStatus>("get_hub_status")
      .then((s) => {
        setHubRoot(s.path);
        setHubSuggestions(s.suggestions);
        setBrowsePath((cur) => {
          if (cur) return cur;
          const landed = bootRef.current;
          if (landed.mode === "browse" && landed.browseKind === "hub") return s.path || null;
          return cur;
        });
      })
      .catch(() => undefined);
  }, []);

  useEffect(() => {
    setPickedTagDraft("");
    if (!picked) {
      setPickedTags([]);
      return;
    }
    invoke<FinderTagValue[]>("get_finder_tags", { path: picked.path })
      .then(setPickedTags)
      .catch(() => setPickedTags([]));
  }, [picked]);

  useEffect(() => {
    if (viewFolderId) {
      refreshFolderView(viewFolderId).catch((e) => setStatus(String(e)));
      return;
    }
    if (mode === "library") {
      refreshLibrary().catch((e) => setStatus(String(e)));
      return;
    }
    if (mode === "subscribe") {
      refreshSources().catch((e) => setStatus(String(e)));
      refreshSubscribeResults(sourceFilterId).catch((e) => setStatus(String(e)));
      return;
    }
    if (mode === "generate") {
      refreshGenerateResults(genFilterId).catch((e) => setStatus(String(e)));
      return;
    }
    refreshBrowse(browsePath).catch((e) => setStatus(String(e)));
    // eslint-disable-next-line react-hooks/exhaustive-deps -- 仅随导航/筛选变化加载，避免死循环
  }, [mode, folderId, tagFilter, colorFilter, browsePath, browseKind, sourceFilterId, genFilterId, viewFolderId]);

  const importPathsToLibrary = async (
    paths: string[],
    intoFolderId?: string | null,
  ) => {
    if (paths.length === 0) return;
    setBusy(true);
    setStatus(`入库中 ${paths.length} 项…`);
    try {
      const result = await invoke<ImportResult>("import_files", {
        paths,
        folderId:
          intoFolderId !== undefined
            ? intoFolderId
            : mode === "library"
              ? folderId
              : null,
      });
      setStatus(
        `入库完成 run_id=${result.runId} · 成功 ${result.imported} · 跳过 ${result.skipped}` +
          (result.errors.length ? ` · 错误 ${result.errors.length}` : ""),
      );
      setBrowseSelected(new Set());
      if (mode === "library" || intoFolderId) await refreshLibrary();
      else {
        const lib = await invoke<LibraryInfo>("get_library_info");
        setInfo(lib);
      }
    } catch (e) {
      setStatus(`入库失败: ${e}`);
    } finally {
      setBusy(false);
    }
  };

  const bindIndexFromPath = async (kind: "local" | "volumes", rawPath: string) => {
    const path = rawPath.trim();
    if (!path) {
      setStatus("请填写要绑定的文件夹绝对路径");
      return;
    }
    setBusy(true);
    try {
      const roots = await invoke<PinnedFolder[]>("add_index_root", { kind, path });
      if (kind === "local") setLocalRoots(roots);
      else setVolumeRoots(roots);
      setPathAsk(null);
      setMode("browse");
      setBrowseKind(kind);
      setViewFolderId(null);
      setSelected(null);
      setPicked(null);
      setTagFilter(null);
      const virtual = virtualRootOf(kind);
      if (kind === "local") setLocalRoot(virtual);
      else setVolumesRoot(virtual);
      setBrowsePath(virtual);
      const name = folderNameFromPath(path);
      console.info("[inspirex]", { event: "index_root_add", kind, path, count: roots.length });
      setStatus(`已绑定${kind === "local" ? "本地库" : "外置卷"}索引「${name}」· 共 ${roots.length} 个（不复制原文件）`);
      await refreshBrowse(virtual);
    } catch (e) {
      setStatus(`绑定索引失败: ${e}`);
    } finally {
      setBusy(false);
    }
  };

  const unbindIndex = async (kind: "local" | "volumes", path: string, name: string) => {
    try {
      const roots = await invoke<PinnedFolder[]>("remove_index_root", { kind, path });
      if (kind === "local") setLocalRoots(roots);
      else setVolumeRoots(roots);
      const virtual = virtualRootOf(kind);
      if (browsePath === path || browsePath?.startsWith(`${path.replace(/\/$/, "")}/`)) {
        setBrowsePath(virtual);
      }
      console.info("[inspirex]", { event: "index_root_remove", kind, path, count: roots.length });
      setStatus(`已解除索引「${name}」（原文件未删）`);
      if (browseKind === kind) {
        await refreshBrowse(virtual);
      }
    } catch (e) {
      setStatus(`解除索引失败: ${e}`);
    }
  };

  const addDisplayFolderFromPath = async (rawPath: string) => {
    const path = rawPath.trim();
    if (!path) {
      setStatus("请填写文件夹绝对路径");
      return;
    }
    const name = folderNameFromPath(path);
    console.info("[inspirex]", { event: "add_folder", mode, browseKind, path });

    if (mode === "browse" && (browseKind === "local" || browseKind === "volumes")) {
      await bindIndexFromPath(browseKind, path);
      return;
    }

    if (mode === "browse") {
      setPinnedFolders((prev) => {
        if (prev.some((p) => p.path === path)) return prev;
        const next = [...prev, { name, path }];
        savePinnedFolders(next);
        return next;
      });
      const vol = volumesRoot.replace(/\/$/, "");
      const hub = hubRoot.replace(/\/$/, "");
      if (path === vol || path.startsWith(`${vol}/`)) {
        setBrowseKind("volumes");
      } else if (hub && (path === hub || path.startsWith(`${hub}/`))) {
        setBrowseKind("hub");
      } else {
        setBrowseKind("local");
      }
      setTagFilter(null);
      setBrowsePath(path);
      setPathAsk(null);
      setStatus(`已追加常用「${name}」（只展示，不复制）`);
      return;
    }

    const folder = await invoke<Folder>("create_folder", {
      name,
      parentId: folderId,
    });
    setMode("library");
    setFolderId(folder.id);
    setTagFilter(null);
    setColorFilter(null);
    setPathAsk(null);
    setStatus(`正在导入文件夹「${name}」…`);
    await importPathsToLibrary([path], folder.id);
  };

  const bindHubFromPath = async (rawPath: string) => {
    const path = rawPath.trim();
    if (!path) {
      setStatus("请填写云同步文件夹的绝对路径");
      return;
    }
    setBusy(true);
    try {
      const bound = await invoke<string>("set_hub_folder", { path });
      setHubRoot(bound);
      setPathAsk(null);
      setMode("browse");
      setBrowseKind("hub");
      setViewFolderId(null);
      setSelected(null);
      setPicked(null);
      setTagFilter(null);
      setBrowsePath(bound);
      console.info("[inspirex]", { event: "hub_bind", path: bound });
      setStatus(`已绑定中转站 · ${bound}（同步由云盘完成）`);
      await refreshBrowse(bound);
    } catch (e) {
      setStatus(`绑定中转站失败: ${e}`);
    } finally {
      setBusy(false);
    }
  };

  const copyIntoHubFromPaths = async (paths: string[]) => {
    if (paths.length === 0) return;
    setBusy(true);
    setStatus(`正在存入中转站 ${paths.length} 项…`);
    try {
      const dest = browseKind === "hub" ? browsePath ?? hubRoot : hubRoot;
      const result = await invoke<HubCopyResult>("copy_to_hub", {
        paths,
        destDir: dest || null,
      });
      console.info("[inspirex]", {
        event: "hub_copy",
        copied: result.copied,
        skipped: result.skipped,
        dest: result.dest,
      });
      setStatus(
        `已存入中转站 ${result.copied} 张` +
          (result.skipped ? ` · 跳过 ${result.skipped}` : "") +
          (result.errors.length ? ` · 错误 ${result.errors.length}` : "") +
          "（云盘会同步到其他设备）",
      );
      if (browseKind === "hub") {
        await refreshBrowse(browsePath ?? result.dest);
      }
    } catch (e) {
      setStatus(`存入中转站失败: ${e}`);
    } finally {
      setBusy(false);
    }
  };

  const onBindHub = () => {
    console.info("[inspirex]", { event: "folder_picker_open", purpose: "hub", kind: "local" });
    setFolderPicker({ purpose: "hub", kind: "local" });
    setStatus("在弹窗里选择 iCloud / Dropbox 等已同步文件夹");
  };

  const onCopyIntoHub = async () => {
    if (!hubBound) {
      setStatus("请先绑定云同步文件夹");
      await onBindHub();
      return;
    }
    try {
      const pickedFiles = await pickFiles([
        "png", "jpg", "jpeg", "webp", "gif", "mp4", "mov", "m4v", "webm", "mkv",
      ]);
      if (!pickedFiles.ok) {
        if (pickedFiles.reason === "need-path") {
          setPathAsk("hub-files");
          setPathDraft("");
          setStatus("请粘贴要存入中转站的文件绝对路径");
        }
        return;
      }
      await copyIntoHubFromPaths(pickedFiles.paths);
    } catch (e) {
      setStatus(`存入中转站失败: ${e}`);
    }
  };

  const onHubDrop = async (e: ReactDragEvent) => {
    if (browseKind !== "hub" || !hubBound) return;
    e.preventDefault();
    const files = Array.from(e.dataTransfer.files);
    const paths = files
      .map((f) => (f as File & { path?: string }).path)
      .filter((p): p is string => Boolean(p));
    if (paths.length === 0) {
      setStatus("拖入失败：拿不到文件路径，请改用「存入」");
      return;
    }
    await copyIntoHubFromPaths(paths);
  };

  const onImport = async () => {
    setBusy(true);
    setStatus("选择图片…");
    try {
      const pickedFiles = await pickFiles([
        "png", "jpg", "jpeg", "webp", "gif", "mp4", "mov", "m4v", "webm", "mkv",
      ]);
      if (!pickedFiles.ok) {
        setBusy(false);
        if (pickedFiles.reason === "need-path") {
          setPathAsk("files");
          setPathDraft("");
          setStatus("桌面对话框不可用，请粘贴图片/视频的绝对路径后点确定");
          return;
        }
        setStatus("");
        return;
      }
      await importPathsToLibrary(pickedFiles.paths);
    } catch (e) {
      setStatus(`导入失败: ${e}`);
      setBusy(false);
    }
  };

  const onAddDisplayFolder = () => {
    const kind: FolderPickerKind =
      mode === "browse" && browseKind === "volumes" ? "volumes" : "local";
    console.info("[inspirex]", {
      event: "folder_picker_open",
      purpose: "index",
      kind,
      mode,
      browseKind,
    });
    setFolderPicker({ purpose: "index", kind });
    setStatus(
      kind === "volumes" ? "在弹窗里选择外置卷上的文件夹" : "在弹窗里选择要绑定的文件夹",
    );
  };

  const onPickFolder = async (path: string) => {
    const purpose = folderPicker?.purpose;
    setFolderPicker(null);
    try {
      if (purpose === "hub") {
        await bindHubFromPath(path);
        return;
      }
      await addDisplayFolderFromPath(path);
    } catch (e) {
      setStatus(purpose === "hub" ? `绑定中转站失败: ${e}` : `追加文件夹失败: ${e}`);
    }
  };

  const folderPickerCopy = folderPicker
    ? folderPicker.purpose === "hub"
      ? { title: "选择中转站文件夹", confirmLabel: "绑定" }
      : mode === "library"
        ? { title: "选择要导入的文件夹", confirmLabel: "导入" }
        : folderPicker.kind === "volumes"
          ? { title: "选择外置卷文件夹", confirmLabel: "绑定" }
          : { title: "选择本地文件夹", confirmLabel: "绑定" }
    : null;

  const onConfirmPathAsk = async () => {
    try {
      if (pathAsk === "hub-files") {
        const paths = splitPathList(pathDraft);
        if (paths.length === 0) {
          setStatus("请填写至少一个文件路径");
          return;
        }
        setPathAsk(null);
        await copyIntoHubFromPaths(paths);
        return;
      }
      if (pathAsk === "files") {
        const paths = splitPathList(pathDraft);
        if (paths.length === 0) {
          setStatus("请填写至少一个文件路径");
          return;
        }
        setPathAsk(null);
        setBusy(true);
        await importPathsToLibrary(paths);
      }
    } catch (e) {
      setStatus(pathAsk === "hub-files" ? `存入中转站失败: ${e}` : `导入失败: ${e}`);
    }
  };

  const onSearch = async () => {
    if (mode === "browse") {
      const q = query.trim().toLowerCase();
      if (!listing) return;
      if (!q) {
        await refreshBrowse(browsePath);
        return;
      }
      const media = listing.media.filter((m) => m.name.toLowerCase().includes(q));
      const foldersFiltered = listing.folders.filter(
        (f) => !sidebarHidden.has(f.path) && f.name.toLowerCase().includes(q),
      );
      setListing({ ...listing, media, folders: foldersFiltered, totalMedia: media.length });
      setStatus(`当前目录筛选「${query}」· ${media.length} 媒体`);
      return;
    }
    setBusy(true);
    setColorFilter(null);
    setTagFilter(null);
    try {
      const list = await invoke<Asset[]>("search_assets", {
        query: {
          text: query || null,
          folderId,
          useClip,
          limit: 100,
        },
      });
      setAssets(list);
      setStatus(
        query
          ? `搜索「${query}」· ${useClip ? "CLIP" : "FTS"} · ${list.length} 条`
          : `全部 · ${list.length} 条`,
      );
    } catch (e) {
      setStatus(`搜索失败: ${e}`);
    } finally {
      setBusy(false);
    }
  };

  const onColorSearch = async (hex: string) => {
    setMode("library");
    setTagFilter(null);
    setFolderId(null);
    setColorFilter(hex);
    setQuery("");
    setBusy(true);
    try {
      const list = await invoke<Asset[]>("search_assets", {
        query: { colorHex: hex, useClip: false, limit: 200 },
      });
      setAssets(list);
      setStatus(`颜色相近「${hex}」· ${list.length} 条`);
    } catch (e) {
      setStatus(`颜色搜索失败: ${e}`);
    } finally {
      setBusy(false);
    }
  };

  const onCreateFolder = async () => {
    const name = window.prompt(
      "文件夹名称\n将记在灵犀项目中，不会在外置卷或本地库新建目录",
    )?.trim();
    if (!name) return;
    try {
      const folder = await invoke<Folder>("create_folder", { name });
      await refreshFolders();
      setStatus(`已在灵犀创建「${folder.name}」（未改动来源目录）。勾选图片后点旁 + 放入`);
    } catch (e) {
      setStatus(`创建文件夹失败: ${e}`);
    }
  };

  const onDeleteFolder = async (f: Folder, e: MouseEvent) => {
    e.stopPropagation();
    const ok = window.confirm(
      `删除文件夹「${f.name}」？\n仅解除虚拟索引，不会删除原文件。`,
    );
    if (!ok) return;
    try {
      const [unlinked, deleted] = await invoke<[number, number]>("delete_folder", {
        folderId: f.id,
      });
      if (folderId === f.id) setFolderId(null);
      if (viewFolderId === f.id) {
        setViewFolderId(null);
        setFolderAssets([]);
        setFolderMedia([]);
      }
      await refreshFolders();
      if (mode === "library" && !viewFolderId) await refreshLibrary();
      setStatus(`已从灵犀删除分类 · 解除索引 ${unlinked} · 删除记录 ${deleted}（来源文件未动）`);
    } catch (err) {
      setStatus(`删除文件夹失败: ${err}`);
    }
  };

  const openVirtualFolder = (id: string) => {
    setViewFolderId(id);
    setTagFilter(null);
    setColorFilter(null);
    setPicked(null);
    setSelected(null);
  };

  const applyToFolder = async (f: Folder) => {
    const paths =
      browseSelected.size > 0
        ? [...browseSelected]
        : mode === "browse" && picked && !viewFolderId
          ? [picked.path]
          : [];
    const assetIds =
      libSelected.size > 0 ? [...libSelected] : selected ? [selected.id] : [];
    if (paths.length === 0 && assetIds.length === 0) {
      setStatus("请先勾选图片，再点文件夹旁 +（虚拟放入，不复制文件）");
      return;
    }
    try {
      const n = await invoke<number>("add_to_folder", {
        folderId: f.id,
        paths: paths.length ? paths : undefined,
        assetIds: assetIds.length ? assetIds : undefined,
      });
      await refreshFolders();
      if (viewFolderId === f.id) await refreshFolderView(f.id);
      setBrowseSelected(new Set());
      setLibSelected(new Set());
      setStatus(
        n > 0
          ? `已虚拟放入「${f.name}」· ${n} 项（不复制文件）`
          : `「${f.name}」里已有这些项`,
      );
    } catch (e) {
      setStatus(`放入文件夹失败: ${e}`);
    }
  };

  const removeFromFolder = async (opts: { assetId?: string; path?: string }) => {
    if (!viewFolderId) return;
    try {
      await invoke("remove_from_folder", {
        folderId: viewFolderId,
        assetIds: opts.assetId ? [opts.assetId] : undefined,
        paths: opts.path ? [opts.path] : undefined,
      });
      await refreshFolderView(viewFolderId);
      await refreshFolders();
      setStatus("已从文件夹移出（原文件未删）");
    } catch (e) {
      setStatus(`移出失败: ${e}`);
    }
  };

  const toggleLibSelect = (id: string) => {
    setLibSelected((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  };

  const openAssetDetail = (a: Asset) => {
    setPickedGen(null);
    setSelected(a);
    setTagDraft(a.tags.join(", "));
    setNotesDraft(a.notes || "");
    setSourceDraft(a.sourceUrl || "");
  };

  const openGenDetail = (path: string, jobId: string) => {
    setSelected(null);
    setPicked(null);
    setPickedGen({ path, jobId });
    console.info("[inspirex]", {
      event: "gen_preview",
      job_id: jobId,
      file: fileBaseName(path),
    });
  };

  const onSaveTags = async () => {
    if (!selected) return;
    const nextTags = tagDraft
      .split(/[,，]/)
      .map((t) => t.trim())
      .filter(Boolean);
    await invoke("set_asset_tags", { assetId: selected.id, tags: nextTags });
    setSelected({ ...selected, tags: nextTags });
    await refreshLibrary();
    setStatus(`已更新标签: ${nextTags.join(", ") || "(空)"}`);
  };

  const onSaveMeta = async () => {
    if (!selected) return;
    await invoke("update_asset_meta", {
      meta: {
        assetId: selected.id,
        sourceUrl: sourceDraft.trim(),
        notes: notesDraft.trim(),
      },
    });
    setSelected({
      ...selected,
      sourceUrl: sourceDraft.trim(),
      notes: notesDraft.trim(),
    });
    await refreshLibrary();
    setStatus("已保存出处 / 笔记");
  };

  const onMoveFolder = async (nextFolderId: string | null) => {
    if (!selected) return;
    await invoke("update_asset_meta", {
      meta: {
        assetId: selected.id,
        folderId: nextFolderId,
      },
    });
    setSelected({ ...selected, folderId: nextFolderId });
    await refreshLibrary();
    setStatus(nextFolderId ? "已移入文件夹" : "已移出文件夹");
  };

  const onDeleteAsset = useCallback(async () => {
    if (!selected) return;
    const ok = window.confirm(
      `删除素材「${selected.filename}」？\n将删除库记录与本地副本（原导入源文件不受影响）。`,
    );
    if (!ok) return;
    try {
      const result = await invoke<{ deleted: boolean; filesRemoved: number }>("delete_asset_cmd", {
        assetId: selected.id,
        deleteFiles: true,
      });
      setSelected(null);
      await refreshLibrary();
      setStatus(
        result.deleted
          ? `已删除 · 清理文件 ${result.filesRemoved}`
          : "素材不存在或已删除",
      );
    } catch (e) {
      setStatus(`删除失败: ${e}`);
    }
  }, [selected, refreshLibrary]);

  const onDragAsset = async (asset: Asset, e: MouseEvent) => {
    e.preventDefault();
    e.stopPropagation();
    if (!libraryRoot) return;
    if (!hasTauriIpc()) {
      setStatus("拖出需要灵犀窗口");
      return;
    }
    try {
      const abs = await invoke<string>("get_asset_abs_path", { assetId: asset.id });
      const icon = asset.thumbPath
        ? absInLibrary(libraryRoot, asset.thumbPath)
        : abs;
      setStatus(`拖出中: ${asset.filename}`);
      await startDrag(
        { item: [abs], icon, mode: "copy" },
        (payload) => {
          setStatus(
            payload.result === "Dropped"
              ? `已拖出: ${asset.filename}`
              : `拖出取消: ${asset.filename}`,
          );
        },
      );
    } catch (err) {
      setStatus(`拖出失败: ${err}`);
    }
  };

  const onRevealAsset = async () => {
    if (!selected || !libraryRoot) return;
    try {
      const abs = await invoke<string>("get_asset_abs_path", { assetId: selected.id });
      if (!hasTauriIpc()) {
        setStatus("在 Finder 显示需要灵犀窗口");
        return;
      }
      await revealItemInDir(abs);
      setStatus(`已在 Finder 显示: ${selected.filename}`);
    } catch (e) {
      setStatus(`显示失败: ${e}`);
    }
  };

  const onRevealGen = async (path: string) => {
    try {
      if (!hasTauriIpc()) {
        setStatus("在 Finder 显示需要灵犀窗口");
        return;
      }
      await revealItemInDir(path);
      setStatus(`已在 Finder 显示: ${fileBaseName(path)}`);
    } catch (e) {
      setStatus(`显示失败: ${e}`);
    }
  };

  const onCreateSource = async () => {
    const url = sourceUrl.trim();
    if (!url) {
      setStatus("请填写 RSS、网页地址或图片 URL 列表");
      return;
    }
    setBusy(true);
    try {
      const src = await invoke<Source>("create_source", {
        req: {
          name: sourceName.trim() || url,
          kind: sourceKind,
          url,
          intervalMin: sourceInterval,
          folderId,
        },
      });
      setSourceName("");
      setSourceUrl("");
      setStatus(`已添加订阅「${src.name}」，正在拉取…`);
      const result = await invoke<ImportResult>("pull_source", { sourceId: src.id });
      setSourceFilterId(src.id);
      await refreshSources();
      await refreshSubscribeResults(src.id);
      setStatus(
        `订阅拉取完成 run_id=${result.runId} · 成功 ${result.imported} · 跳过 ${result.skipped}` +
          (result.errors.length ? ` · 错误 ${result.errors.length}` : ""),
      );
    } catch (e) {
      setStatus(`添加订阅失败: ${e}`);
    } finally {
      setBusy(false);
    }
  };

  const onPullSource = async (id: string) => {
    setBusy(true);
    try {
      const result = await invoke<ImportResult>("pull_source", { sourceId: id });
      setSourceFilterId(id);
      await refreshSources();
      await refreshSubscribeResults(id);
      setStatus(
        `拉取完成 run_id=${result.runId} · 成功 ${result.imported} · 跳过 ${result.skipped}` +
          (result.errors.length ? ` · 错误 ${result.errors.length}` : ""),
      );
    } catch (e) {
      setStatus(`拉取失败: ${e}`);
    } finally {
      setBusy(false);
    }
  };

  const onDeleteSource = async (src: Source) => {
    if (!window.confirm(`删除订阅「${src.name}」？已入库素材保留。`)) return;
    await invoke("delete_source", { sourceId: src.id });
    if (sourceFilterId === src.id) setSourceFilterId(null);
    await refreshSources();
    await refreshSubscribeResults(sourceFilterId === src.id ? null : sourceFilterId);
    setStatus(`已删除订阅「${src.name}」`);
  };

  const onToggleSource = async (src: Source) => {
    await invoke("set_source_enabled", { sourceId: src.id, enabled: !src.enabled });
    await refreshSources();
  };

  const onGenerate = async () => {
    const prompt = genPrompt.trim();
    if (!prompt) {
      setStatus("请填写生图描述");
      return;
    }
    setBusy(true);
    setStatus(`生成中 n=${genN}…`);
    try {
      const job = await invoke<GenJob>("generate_images", {
        req: {
          prompt,
          n: genN,
          size: genSize,
          folderId,
        },
      });
      setGenFilterId(job.id);
      await refreshGenerateResults(job.id);
      setStatus(
        `已生成 job=${job.id} · ${job.provider} · ${job.outputPaths?.length ?? 0} 张，尚未入库` +
          (job.error ? ` · ${job.error}` : ""),
      );
    } catch (e) {
      await refreshGenerateResults(genFilterId);
      setStatus(`生成失败: ${e}`);
    } finally {
      setBusy(false);
    }
  };

  const importableJob = useMemo(() => {
    const selected = genJobs.find((j) => j.id === genFilterId);
    if (selected?.status === "generated" && (selected.outputPaths?.length ?? 0) > 0) {
      return selected;
    }
    return genJobs.find((j) => j.status === "generated" && (j.outputPaths?.length ?? 0) > 0) ?? null;
  }, [genJobs, genFilterId]);

  const genJob = genJobs.find((j) => j.id === genFilterId) ?? null;
  const genStaging = genJob?.status === "generated" ? genJob.outputPaths ?? [] : [];
  const pickedGenJob = pickedGen
    ? genJobs.find((j) => j.id === pickedGen.jobId) ?? null
    : null;

  const onImportGen = async (jobId?: string) => {
    const id = jobId || importableJob?.id;
    if (!id) {
      setStatus("请先生成，或选中一条「已生成」任务再入库");
      return;
    }
    setBusy(true);
    setStatus("入库中…");
    try {
      const job = await invoke<GenJob>("import_gen_job", {
        jobId: id,
        folderId,
      });
      setGenFilterId(job.id);
      setPickedGen(null);
      await refreshGenerateResults(job.id);
      setStatus(
        `入库完成 job=${job.id} · ${job.imported} 张` + (job.error ? ` · ${job.error}` : ""),
      );
    } catch (e) {
      await refreshGenerateResults(genFilterId);
      setStatus(`入库失败: ${e}`);
    } finally {
      setBusy(false);
    }
  };

  const toggleBrowseSelect = (path: string) => {
    setBrowseSelected((prev) => {
      const next = new Set(prev);
      if (next.has(path)) next.delete(path);
      else next.add(path);
      return next;
    });
    browseAnchorRef.current = path;
  };

  const selectBrowseRange = (toPath: string) => {
    const paths = listing?.media.map((m) => m.path) ?? [];
    const anchor = browseAnchorRef.current ?? toPath;
    const i1 = paths.indexOf(anchor);
    const i2 = paths.indexOf(toPath);
    if (i1 < 0 || i2 < 0) {
      setBrowseSelected(new Set([toPath]));
      browseAnchorRef.current = toPath;
      return;
    }
    const lo = Math.min(i1, i2);
    const hi = Math.max(i1, i2);
    setBrowseSelected(new Set(paths.slice(lo, hi + 1)));
  };

  const onBrowseMediaClick = (m: ExternalItem, e: MouseEvent) => {
    e.preventDefault();
    if (e.shiftKey) {
      selectBrowseRange(m.path);
      setPicked(m);
      return;
    }
    if (e.metaKey || e.ctrlKey) {
      toggleBrowseSelect(m.path);
      setPicked(m);
      return;
    }
    setBrowseSelected(new Set([m.path]));
    browseAnchorRef.current = m.path;
    setPicked(m);
  };

  const closeDetail = useCallback(() => {
    setPicked(null);
    setSelected(null);
    setPickedGen(null);
  }, []);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const el = e.target as HTMLElement | null;
      if (
        el &&
        (el.tagName === "INPUT" ||
          el.tagName === "TEXTAREA" ||
          el.tagName === "SELECT" ||
          el.isContentEditable)
      ) {
        return;
      }
      if (e.key === "Escape") {
        if (picked || selected || pickedGen) {
          closeDetail();
          return;
        }
        if (browseSelected.size > 0 || libSelected.size > 0) {
          setBrowseSelected(new Set());
          setLibSelected(new Set());
        }
        return;
      }
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "a" && mode === "browse") {
        e.preventDefault();
        const all = listing?.media.map((m) => m.path) ?? [];
        setBrowseSelected(new Set(all));
        setStatus(`已全选 ${all.length} 项`);
        return;
      }
      if (
        (e.key === "Delete" || e.key === "Backspace") &&
        mode === "library" &&
        selected
      ) {
        e.preventDefault();
        onDeleteAsset().catch(() => undefined);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [
    closeDetail,
    mode,
    browseSelected.size,
    libSelected.size,
    listing,
    selected,
    picked,
    pickedGen,
    onDeleteAsset,
  ]);

  const onLibCardPointerDown = (asset: Asset, e: ReactPointerEvent) => {
    if (e.button !== 0) return;
    libDragRef.current = { asset, x: e.clientX, y: e.clientY };
    libDragStarted.current = false;
  };

  const onLibCardPointerMove = (e: ReactPointerEvent) => {
    const s = libDragRef.current;
    if (!s || libDragStarted.current) return;
    const dx = e.clientX - s.x;
    const dy = e.clientY - s.y;
    if (dx * dx + dy * dy < 36) return;
    libDragStarted.current = true;
    libDragRef.current = null;
    onDragAsset(s.asset, e as unknown as MouseEvent).catch(() => undefined);
  };

  const onLibCardPointerUp = (asset: Asset) => {
    if (libDragRef.current && !libDragStarted.current) {
      openAssetDetail(asset);
    }
    libDragRef.current = null;
    libDragStarted.current = false;
  };

  const onLibCardPointerCancel = () => {
    libDragRef.current = null;
    libDragStarted.current = false;
  };

  const crumb = useMemo(() => {
    if (!browsePath) return [];
    if (browseKind === "local" || browseKind === "volumes") {
      const virtual = browseKind === "local" ? localRoot || LOCAL_VIRTUAL : volumesRoot || VOLUMES_VIRTUAL;
      const roots = browseKind === "local" ? localRoots : volumeRoots;
      const items = [{ name: sourceLabel, path: virtual }];
      if (isKindVirtualPath(browsePath, virtual)) return items;
      const bound = roots.find((r) => {
        const root = r.path.replace(/\/$/, "");
        return browsePath === root || browsePath.startsWith(`${root}/`);
      });
      if (bound) {
        items.push({ name: bound.name, path: bound.path });
        const rest = browsePath.slice(bound.path.replace(/\/$/, "").length).replace(/^\//, "");
        let acc = bound.path.replace(/\/$/, "");
        for (const p of rest ? rest.split("/") : []) {
          acc = `${acc}/${p}`;
          items.push({ name: displayFolderName(p), path: acc });
        }
        return items;
      }
      return [{ name: browsePath, path: browsePath }];
    }
    const root = browseRoot.replace(/\/$/, "");
    if (!root || !browsePath.startsWith(root)) return [{ name: browsePath, path: browsePath }];
    const rest = browsePath.slice(root.length).replace(/^\//, "");
    const parts = rest ? rest.split("/") : [];
    const items = [{ name: sourceLabel, path: root }];
    let acc = root;
    for (const p of parts) {
      acc = `${acc}/${p}`;
      items.push({ name: displayFolderName(p), path: acc });
    }
    return items;
  }, [browsePath, browseRoot, browseKind, sourceLabel, localRoot, localRoots, volumesRoot, volumeRoots]);

  const headerMeta = useMemo(() => {
    if (viewFolderId) {
      const f = folders.find((x) => x.id === viewFolderId);
      const n = folderAssets.length + folderMedia.length;
      return `虚拟文件夹 · ${f?.name ?? ""} · ${n} 项（不复制原文件）`;
    }
    if (mode === "subscribe") {
      return `订阅源 ${sources.length} 条${tagFilter ? ` · 标签 ${tagFilter}` : ""} · 拉取后走同一入库管道`;
    }
    if (mode === "generate") {
      return `AI 生图 · provider=${info?.imageGenProvider ?? "mock"}${tagFilter ? ` · 标签 ${tagFilter}` : ""}`;
    }
    if (mode === "browse") {
      const n = browseSelected.size;
      const tagHint = tagFilter ? ` · 标签 ${tagFilter}` : "";
      return `链接浏览${tagHint}${n ? ` · 已选 ${n}` : ""}（不另存原文件${
        browseKind === "hub"
          ? " · 中转站由云盘多端同步"
          : browsePath?.toLowerCase().includes(".photoslibrary")
            ? " · 本机相册"
            : ""
      }）`;
    }
    if (!info) return "加载中…";
    const filterHint = colorFilter
      ? ` · 色 ${colorFilter}`
      : tagFilter
        ? ` · 标签 ${tagFilter}`
        : "";
    return `${info.assetCount} 素材 · ${info.aiMockMode ? "AI Mock" : "本地 CLIP"} · ${info.clipModel ?? ""} · 采集 :${info.ingestPort ?? 18765}${filterHint}`;
  }, [
    viewFolderId,
    folders,
    folderAssets.length,
    folderMedia.length,
    mode,
    browseKind,
    browseSelected.size,
    browsePath,
    tagFilter,
    info,
    colorFilter,
    sources.length,
  ]);

  return (
    <div className={`app ${detailOpen ? "app-detail-open" : ""}`}>
      <aside className="sidebar">
        <div className="brand">
          <div className="brand-name">灵犀</div>
          <div className="brand-sub">InspireX</div>
        </div>

        <a
          className={`nav-item ${
            mode === "library" && !viewFolderId && folderId === null && !tagFilter && !colorFilter
              ? "active"
              : ""
          }`}
          href={PATHS.library}
          onClick={(e) =>
            spaNav(e, () => {
              setMode("library");
              setViewFolderId(null);
              setPicked(null);
              setFolderId(null);
              setColorFilter(null);
              setQuery("");
            })
          }
        >
          全部素材
        </a>
        <div className="nav-label">来源</div>
        <a
          className={`nav-item ${mode === "browse" && browseKind === "volumes" && !viewFolderId ? "active" : ""}`}
          href={PATHS.volumes}
          onClick={(e) =>
            spaNav(e, () => {
              setMode("browse");
              setBrowseKind("volumes");
              setViewFolderId(null);
              setSelected(null);
              setPicked(null);
              if (browseKind === "volumes" && browsePath === volumesRoot) {
                refreshBrowse(volumesRoot || VOLUMES_VIRTUAL).catch((err) => setStatus(String(err)));
              } else {
                setBrowsePath(volumesRoot || VOLUMES_VIRTUAL);
              }
            })
          }
        >
          外置卷
        </a>
        <a
          className={`nav-item ${mode === "browse" && browseKind === "local" && !viewFolderId ? "active" : ""}`}
          href={PATHS.local}
          onClick={(e) =>
            spaNav(e, () => {
              setMode("browse");
              setBrowseKind("local");
              setViewFolderId(null);
              setSelected(null);
              setPicked(null);
              if (browseKind === "local" && browsePath === localRoot) {
                refreshBrowse(localRoot || LOCAL_VIRTUAL).catch((err) => setStatus(String(err)));
              } else {
                setBrowsePath(localRoot || LOCAL_VIRTUAL);
              }
            })
          }
        >
          本地库
        </a>
        <a
          className={`nav-item ${mode === "browse" && browseKind === "hub" && !viewFolderId ? "active" : ""}`}
          href={PATHS.hub}
          onClick={(e) =>
            spaNav(e, () => {
              setMode("browse");
              setBrowseKind("hub");
              setViewFolderId(null);
              setSelected(null);
              setPicked(null);
              if (browseKind === "hub" && (browsePath === hubRoot || (!hubRoot && !browsePath))) {
                refreshBrowse(hubRoot || null).catch((err) => setStatus(String(err)));
              } else {
                setBrowsePath(hubRoot || null);
              }
            })
          }
        >
          中转站
        </a>
        <a
          className={`nav-item ${mode === "subscribe" && !viewFolderId ? "active" : ""}`}
          href={PATHS.subscribe}
          onClick={(e) =>
            spaNav(e, () => {
              setMode("subscribe");
              setViewFolderId(null);
              setSelected(null);
              setPicked(null);
            })
          }
        >
          订阅源
        </a>
        <a
          className={`nav-item ${mode === "generate" && !viewFolderId ? "active" : ""}`}
          href={PATHS.generate}
          onClick={(e) =>
            spaNav(e, () => {
              setMode("generate");
              setViewFolderId(null);
              setSelected(null);
              setPicked(null);
            })
          }
        >
          AI 生图
        </a>

        {mode === "browse" ? (
          <>
            <div className="nav-label">
              {browseKind === "local" || browseKind === "volumes" ? "索引目录" : "常用"}
              <button
                type="button"
                className="linkish"
                title={
                  browseKind === "local" || browseKind === "volumes"
                    ? "绑定要索引的文件夹"
                    : "追加要展示的文件夹"
                }
                onClick={onAddDisplayFolder}
              >
                +
              </button>
            </div>
            {suggestedVisible.map((p) => {
              const indexed =
                (browseKind === "local" && localRoots.some((r) => r.path === p.path)) ||
                (browseKind === "volumes" && volumeRoots.some((r) => r.path === p.path));
              return (
              <div
                key={p.path}
                className={`nav-row ${browsePath === p.path ? "active" : ""}`}
              >
                <a
                  className="nav-item"
                  href={toHref({
                    mode: "browse",
                    browseKind: volumeRoots.some(
                      (r) =>
                        p.path === r.path ||
                        p.path.startsWith(`${r.path.replace(/\/$/, "")}/`),
                    )
                      ? "volumes"
                      : hubRoot &&
                          (p.path === hubRoot ||
                            p.path.startsWith(`${hubRoot.replace(/\/$/, "")}/`))
                        ? "hub"
                        : "local",
                    viewFolderId: null,
                    browsePath: p.path,
                    tagFilter: null,
                    colorFilter: null,
                  })}
                  onClick={(e) => {
                    spaNav(e, () => {
                      setViewFolderId(null);
                      setTagFilter(null);
                      if (volumeRoots.some((r) => p.path === r.path || p.path.startsWith(r.path.replace(/\/$/, "") + "/"))) {
                        setBrowseKind("volumes");
                      } else if (hubRoot && (p.path === hubRoot || p.path.startsWith(hubRoot.replace(/\/$/, "") + "/"))) {
                        setBrowseKind("hub");
                      } else {
                        setBrowseKind("local");
                      }
                      setBrowsePath(p.path);
                    });
                  }}
                >
                  {p.name}
                </a>
                <button
                  type="button"
                  className="nav-del"
                  title={indexed ? "解除索引（不删原文件）" : "从侧栏隐藏（不删原文件）"}
                  aria-label={`隐藏 ${p.name}`}
                  onClick={(e) => {
                    e.stopPropagation();
                    if (browseKind === "local" && localRoots.some((r) => r.path === p.path)) {
                      unbindIndex("local", p.path, p.name).catch(() => undefined);
                      return;
                    }
                    if (browseKind === "volumes" && volumeRoots.some((r) => r.path === p.path)) {
                      unbindIndex("volumes", p.path, p.name).catch(() => undefined);
                      return;
                    }
                    hideFromSidebar(p.path, p.name);
                  }}
                >
                  ×
                </button>
              </div>
              );
            })}
          </>
        ) : null}

        <div className="nav-label">
          文件夹
          <button
            type="button"
            className="linkish"
            title="在灵犀中新建分类（不在来源磁盘上建目录）"
            onClick={onCreateFolder}
          >
            +
          </button>
        </div>
        {folders.length === 0 ? (
          <div className="nav-hint">记在灵犀里，不会改动外置卷 / 本地库 / 中转站</div>
        ) : null}
        {folders.map((f) => (
          <div
            key={f.id}
            className={`nav-row ${viewFolderId === f.id ? "active" : ""}`}
          >
            <a
              className="nav-item"
              href={folderPath(f.id)}
              onClick={(e) => spaNav(e, () => openVirtualFolder(f.id))}
            >
              {f.parentId ? `└ ${f.name}` : f.name}
              {f.itemCount ? (
                <span className="nav-count">{f.itemCount}</span>
              ) : null}
            </a>
            <button
              type="button"
              className="nav-tag-apply"
              title="把已选图片虚拟放入灵犀分类（不复制文件）"
              aria-label={`放入 ${f.name}`}
              onClick={() => applyToFolder(f)}
            >
              +
            </button>
            <button
              type="button"
              className="nav-del"
              title="删除灵犀分类（不删来源文件）"
              aria-label={`删除 ${f.name}`}
              onClick={(e) => onDeleteFolder(f, e)}
            >
              ×
            </button>
          </div>
        ))}

        {mode === "browse" ? (
          <>
            {!sourceNavRoot && !tagFilter && browseKind !== "local" && browseKind !== "volumes" ? (
              <>
                <div className="nav-label">
                  {browseKind === "hub" ? "中转站目录" : "已挂载"}
                  {sidebarHidden.size > 0 ? (
                    <button
                      type="button"
                      className="linkish linkish-sm"
                      title="恢复侧栏已隐藏项"
                      onClick={restoreSidebarHidden}
                    >
                      恢复{sidebarHidden.size}
                    </button>
                  ) : null}
                </div>
                {visibleBrowseFolders.map((f) => (
                  <div
                    key={f.path}
                    className={`nav-row ${browsePath === f.path ? "active" : ""}`}
                  >
                    <a
                      className="nav-item"
                      href={toHref({
                        mode: "browse",
                        browseKind,
                        viewFolderId: null,
                        browsePath: f.path,
                        tagFilter: null,
                        colorFilter: null,
                      })}
                      onClick={(e) =>
                        spaNav(e, () => {
                          setViewFolderId(null);
                          setTagFilter(null);
                          setBrowsePath(f.path);
                        })
                      }
                    >
                      {f.name}
                    </a>
                    <button
                      type="button"
                      className="nav-del"
                      title="从侧栏隐藏（不删原文件）"
                      aria-label={`隐藏 ${f.name}`}
                      onClick={(e) => {
                        e.stopPropagation();
                        hideFromSidebar(f.path, f.name);
                      }}
                    >
                      ×
                    </button>
                  </div>
                ))}
              </>
            ) : null}
            {sourceNavRoot && listing?.parent && !tagFilter ? (
              <a
                className="nav-item"
                href={toHref({
                  mode: "browse",
                  browseKind,
                  viewFolderId: null,
                  browsePath: listing.parent,
                  tagFilter: null,
                  colorFilter: null,
                })}
                onClick={(e) =>
                  spaNav(e, () => {
                    setViewFolderId(null);
                    setTagFilter(null);
                    setBrowsePath(listing.parent!);
                  })
                }
              >
                ↑ 上级
              </a>
            ) : null}
          </>
        ) : null}
            <div className="nav-label">
              标签
              <span className="nav-label-actions">
                {hiddenFinderTags.size > 0 ? (
                  <button
                    type="button"
                    className="linkish linkish-sm"
                    title="恢复已从边栏移除的标签"
                    onClick={restoreSidebarHidden}
                  >
                    恢复{hiddenFinderTags.size}
                  </button>
                ) : null}
                <button
                  type="button"
                  className="linkish"
                  title="新建标签"
                  onClick={onCreateTag}
                >
                  +
                </button>
              </span>
            </div>
            {visibleFinderTags.map((t) => {
              const isCustom = customFinderTags.some((c) => c.name === t.name);
              return (
                <div
                  key={t.name}
                  className={`nav-row ${tagFilter === t.name ? "active" : ""}`}
                >
                  <button
                    type="button"
                    className="nav-item nav-tag"
                    onClick={() => {
                      setViewFolderId(null);
                      setTagFilter((cur) => (cur === t.name ? null : t.name));
                      setFolderId(null);
                      setColorFilter(null);
                    }}
                  >
                    <span
                      className="tag-dot"
                      style={{ background: t.hex }}
                      title={isCustom ? "点击切换颜色（与 Finder 色板相同）" : t.name}
                      onClick={
                        isCustom
                          ? (e) => {
                              e.stopPropagation();
                              cycleFinderTagColor(t.name);
                            }
                          : undefined
                      }
                    />
                    {t.name}
                  </button>
                  <button
                    type="button"
                    className="nav-tag-apply"
                    title="贴到已选 / 当前素材"
                    aria-label={`给已选文件加上 ${t.name}`}
                    onClick={() => applyFinderTagToSelection(t)}
                  >
                    +
                  </button>
                  <button
                    type="button"
                    className="nav-del"
                    title="从边栏移除（不删文件上的标签）"
                    aria-label={`从边栏移除 ${t.name}`}
                    onClick={() => hideFinderTag(t.name)}
                  >
                    ×
                  </button>
                </div>
              );
            })}
      </aside>

      <main className="main">
        <header className="toolbar">
          {viewFolderId ? (
            <div className="crumbs">
              <a
                className="crumb"
                href={toHref({
                  mode,
                  browseKind,
                  viewFolderId: null,
                  browsePath,
                  tagFilter: null,
                  colorFilter: null,
                })}
                onClick={(e) => spaNav(e, () => setViewFolderId(null))}
              >
                返回
              </a>
              <span className="crumb-sep">/</span>
              <span className="crumb">
                {folders.find((f) => f.id === viewFolderId)?.name ?? "文件夹"}
              </span>
            </div>
          ) : mode === "browse" ? (
            <div className="crumbs">
              {crumb.map((c, i) => (
                <span key={c.path}>
                  {i > 0 ? <span className="crumb-sep">/</span> : null}
                  <a
                    className="crumb"
                    href={toHref({
                      mode: "browse",
                      browseKind,
                      viewFolderId: null,
                      browsePath: c.path,
                      tagFilter: null,
                      colorFilter: null,
                    })}
                    onClick={(e) =>
                      spaNav(e, () => {
                        setTagFilter(null);
                        setViewFolderId(null);
                        setBrowsePath(c.path);
                      })
                    }
                  >
                    {c.name}
                  </a>
                </span>
              ))}
            </div>
          ) : null}
          {viewFolderId ? null : mode === "subscribe" || mode === "generate" ? (
            <div className="search-row">
              <div className="panel-title">
                {mode === "subscribe" ? "订阅 / RSS" : "AI 生图"}
              </div>
              {mode === "subscribe" ? (
                <button type="button" onClick={() => refreshSources()} disabled={busy}>
                  刷新
                </button>
              ) : (
                <button
                  type="button"
                  onClick={() => refreshGenerateResults(genFilterId)}
                  disabled={busy}
                >
                  刷新结果
                </button>
              )}
            </div>
          ) : (
            <div className="search-row">
              <input
                value={query}
                onChange={(e) => setQuery(e.target.value)}
                onKeyDown={(e) => e.key === "Enter" && onSearch()}
                placeholder={
                  mode === "browse"
                    ? "筛选当前目录文件名…"
                    : useClip
                      ? `以文搜图（${info?.aiMockMode === false ? "本地 CLIP" : "Mock CLIP"}）…`
                      : "文件名 / 标签全文搜索…"
                }
              />
              {mode === "library" ? (
                <label className="clip-toggle">
                  <input
                    type="checkbox"
                    checked={useClip}
                    onChange={(e) => setUseClip(e.target.checked)}
                  />
                  CLIP
                </label>
              ) : null}
              <button type="button" onClick={onSearch} disabled={busy}>
                {mode === "browse" ? "筛选" : "搜索"}
              </button>
              {mode === "library" ? (
                <>
                  <button type="button" className="primary" onClick={onImport} disabled={busy}>
                    导入
                  </button>
                  <button type="button" onClick={onAddDisplayFolder} disabled={busy} title="选择文件夹，作为库内目录导入并展示">
                    追加文件夹
                  </button>
                </>
              ) : (
                <>
                  {browseKind === "hub" ? (
                    <>
                      <button
                        type="button"
                        className="primary"
                        onClick={onCopyIntoHub}
                        disabled={busy || !hubBound}
                        title="复制图片到云同步文件夹，其他设备可见"
                      >
                        存入
                      </button>
                      <button
                        type="button"
                        onClick={onBindHub}
                        disabled={busy}
                        title="绑定 iCloud / Dropbox 等已同步的文件夹"
                      >
                        {hubBound ? "更换绑定" : "绑定文件夹"}
                      </button>
                    </>
                  ) : hubBound && browseSelected.size > 0 ? (
                    <button
                      type="button"
                      onClick={() => copyIntoHubFromPaths([...browseSelected])}
                      disabled={busy}
                      title="把已选图片复制到中转站，云盘会同步到其他设备"
                    >
                      复制到中转站
                    </button>
                  ) : null}
                  <button type="button" onClick={onAddDisplayFolder} disabled={busy} title={browseKind === "local" || browseKind === "volumes" ? "绑定文件夹到当前来源（可多个，只索引不复制）" : "把文件夹加到侧栏常用"}>
                    追加文件夹
                  </button>
                  <button
                    type="button"
                    onClick={() => refreshBrowse(browsePath)}
                    disabled={busy}
                  >
                    刷新
                  </button>
                </>
              )}
            </div>
          )}
          <div className="meta">{headerMeta}</div>
          {pathAsk ? (
            <div className="path-ask">
              <input
                value={pathDraft}
                onChange={(e) => setPathDraft(e.target.value)}
                onKeyDown={(e) => e.key === "Enter" && onConfirmPathAsk()}
                placeholder="文件绝对路径，逗号或换行分隔"
                autoFocus
              />
              <button type="button" className="primary" onClick={onConfirmPathAsk} disabled={busy}>
                确定
              </button>
              <button
                type="button"
                onClick={() => {
                  setPathAsk(null);
                  setStatus("");
                }}
              >
                取消
              </button>
            </div>
          ) : null}
          {status ? <div className="status">{status}</div> : null}
        </header>

        {viewFolderId ? (
          <section className="grid">
            {folderMedia.length === 0 && folderAssets.length === 0 ? (
              <div className="empty">
                <p>这个文件夹还是空的。</p>
                <p className="hint">
                  勾选图片后点侧栏文件夹旁的 +，即可虚拟放入（不复制、不移动原文件）。
                </p>
              </div>
            ) : (
              <>
                {folderMedia.map((m) => (
                  <div
                    key={m.path}
                    className={`card ${picked?.path === m.path ? "card-active" : ""}`}
                  >
                    <button
                      type="button"
                      className="card-remove"
                      title="从文件夹移出（不删原文件）"
                      aria-label={`移出 ${m.name}`}
                      onClick={(e) => {
                        e.stopPropagation();
                        removeFromFolder({ path: m.path });
                      }}
                    >
                      ×
                    </button>
                    <button
                      type="button"
                      className="card-body"
                      onClick={() => setPicked(m)}
                    >
                      <ExternalThumb item={m} />
                      <div className="card-meta">
                        <span className="name">{m.name}</span>
                        <span className="badge">链接</span>
                      </div>
                    </button>
                  </div>
                ))}
                {folderAssets.map((a) => (
                  <div
                    key={a.id}
                    className={`card ${selected?.id === a.id ? "card-active" : ""} ${libSelected.has(a.id) ? "card-selected" : ""}`}
                    style={{ borderColor: a.dominantColor || "transparent" }}
                  >
                    <button
                      type="button"
                      className="card-remove"
                      title="从文件夹移出（不删原文件）"
                      aria-label={`移出 ${a.filename}`}
                      onClick={(e) => {
                        e.stopPropagation();
                        removeFromFolder({ assetId: a.id });
                      }}
                    >
                      ×
                    </button>
                    <button
                      type="button"
                      className="card-body"
                      onPointerDown={(e) => onLibCardPointerDown(a, e)}
                      onPointerMove={onLibCardPointerMove}
                      onPointerUp={() => onLibCardPointerUp(a)}
                      onPointerCancel={onLibCardPointerCancel}
                      title="单击详情 · 拖动可拖出"
                    >
                      <RatioThumb
                        src={libraryRoot ? assetSrc(libraryRoot, a) : null}
                        alt={a.filename}
                        cacheKey={a.id}
                        width={a.width}
                        height={a.height}
                        background={a.dominantColor || undefined}
                      />
                      <div className="card-meta">
                        <span className="name">{a.filename}</span>
                        <span className="badge">库</span>
                      </div>
                    </button>
                  </div>
                ))}
              </>
            )}
          </section>
        ) : mode === "browse" ? (
          <section
            className={`grid ${browseKind === "hub" ? "hub-drop" : ""}`}
            onDragOver={
              browseKind === "hub" && hubBound
                ? (e) => {
                    e.preventDefault();
                    e.dataTransfer.dropEffect = "copy";
                  }
                : undefined
            }
            onDrop={browseKind === "hub" && hubBound ? onHubDrop : undefined}
          >
            {browseKind === "hub" && !hubBound ? (
              <div className="empty">
                <p>绑定一个云同步文件夹作为中转站</p>
                <p className="hint">
                  用 iCloud、Dropbox、OneDrive 等已在多台电脑同步的目录。图片存入后由云盘同步，灵犀不另建云服务。
                </p>
                <button type="button" className="primary" onClick={onBindHub} disabled={busy}>
                  选择文件夹
                </button>
                {hubSuggestions.length > 0 ? (
                  <div className="hub-suggest">
                    {hubSuggestions.map((s) => (
                      <button
                        key={s.path}
                        type="button"
                        onClick={() => bindHubFromPath(s.path)}
                        disabled={busy}
                        title={s.path}
                      >
                        使用 {s.name}
                      </button>
                    ))}
                  </div>
                ) : null}
              </div>
            ) : browseError && !listing ? (
              <div className="empty">
                <p>无法连接{sourceLabel}</p>
                <p className="hint">{browseError}</p>
                <p className="hint">{browsePath ?? browseRoot}</p>
                <button
                  type="button"
                  className="primary"
                  onClick={() => refreshBrowse(browsePath ?? browseRoot)}
                  disabled={busy}
                >
                  重试
                </button>
                {browseKind === "hub" ? (
                  <button type="button" onClick={onBindHub} disabled={busy}>
                    更换绑定
                  </button>
                ) : null}
              </div>
            ) : !listing ? (
              <div className="empty">
                <p>正在连接{sourceLabel}…</p>
                <p className="hint">{browsePath ?? browseRoot}</p>
              </div>
            ) : visibleBrowseFolders.length === 0 && listing.media.length === 0 ? (
              <div className="empty">
                <p>
                  {tagFilter
                    ? `没有带「${tagFilter}」标签的图片/视频。`
                    : sourceNavRoot
                      ? "此目录下没有图片/视频（可能是空文件夹）。"
                      : browseKind === "local"
                        ? localRoots.length === 0
                          ? "还没有绑定索引目录。"
                          : "本机索引目录下没有可显示的内容。"
                        : browseKind === "volumes"
                          ? volumeRoots.length === 0
                            ? "还没有绑定外置卷索引。"
                            : "外置卷索引目录下没有可显示的内容。"
                        : browseKind === "hub"
                          ? "中转站还是空的。"
                          : "未检测到外接存储。"}
                </p>
                <p className="hint">
                  {tagFilter
                    ? "勾选文件后点侧栏标签旁的 +，即可像 Finder 一样贴上标签。"
                    : sourceNavRoot
                      ? "只展示来源文件，不另存副本。标签在左侧，与 Finder 相同。"
                      : browseKind === "local"
                        ? "点右上角「追加文件夹」，绑定照片、图片、影片等本机目录。只做索引，不复制原文件。"
                        : browseKind === "volumes"
                          ? "点右上角「追加文件夹」，绑定移动硬盘或 U 盘上的素材目录。只做索引，不复制原文件。"
                        : browseKind === "hub"
                          ? "点「存入」或把图片拖进这里，云盘会同步到其他设备。"
                          : "插入 U 盘、移动硬盘或挂载网络卷后点刷新。"}
                </p>
                {browseKind === "hub" && !tagFilter ? (
                  <button type="button" className="primary" onClick={onCopyIntoHub} disabled={busy}>
                    存入图片
                  </button>
                ) : null}
                {(browseKind === "local" || browseKind === "volumes") && !tagFilter ? (
                  <button type="button" className="primary" onClick={onAddDisplayFolder} disabled={busy}>
                    追加文件夹
                  </button>
                ) : null}
              </div>
            ) : (
              <>
                {visibleBrowseFolders.map((f) => (
                  <button
                    key={f.path}
                    type="button"
                    className="card folder-card"
                    onClick={() => {
                      setTagFilter(null);
                      setBrowsePath(f.path);
                    }}
                  >
                    <div className="thumb folder-thumb">文件夹</div>
                    <div className="card-meta">
                      <span className="name">{f.name}</span>
                      <span className="badge">DIR</span>
                    </div>
                  </button>
                ))}
                {listing.media.map((m) => (
                  <div
                    key={m.path}
                    className={`card ${picked?.path === m.path ? "card-active" : ""} ${browseSelected.has(m.path) ? "card-selected" : ""}`}
                  >
                    <button
                      type="button"
                      className="card-check"
                      title="勾选 / 取消"
                      onClick={(e) => {
                        e.stopPropagation();
                        if (e.shiftKey) {
                          selectBrowseRange(m.path);
                          return;
                        }
                        toggleBrowseSelect(m.path);
                      }}
                    >
                      {browseSelected.has(m.path) ? "✓" : ""}
                    </button>
                    <button
                      type="button"
                      className="card-body"
                      onClick={(e) => onBrowseMediaClick(m, e)}
                    >
                      <ExternalThumb item={m} />
                      <div className="card-meta">
                        <span className="name">{m.name}</span>
                        <span className="badge">{m.kind === "video" ? "VIDEO" : "IMG"}</span>
                      </div>
                    </button>
                  </div>
                ))}
                {listing.truncated ? (
                  <div className="load-more">
                    <button type="button" onClick={loadMoreBrowse} disabled={busy}>
                      {busy
                        ? "加载中…"
                        : `加载更多（已 ${listing.media.length}/${listing.totalMedia}）`}
                    </button>
                  </div>
                ) : null}
              </>
            )}
          </section>
        ) : mode === "subscribe" ? (
          <section className="panel panel-split">
            <div className="panel-left">
              <div className="form-card">
              <label className="tag-label">
                名称
                <input
                  value={sourceName}
                  onChange={(e) => setSourceName(e.target.value)}
                  placeholder="例如 Unsplash 建筑"
                />
              </label>
              <label className="tag-label">
                类型
                <select
                  value={sourceKind}
                  onChange={(e) => setSourceKind(e.target.value as "rss" | "url_list" | "html_page")}
                >
                  <option value="rss">RSS / Atom</option>
                  <option value="html_page">公开网页（抽图片）</option>
                  <option value="url_list">图片 URL 列表</option>
                </select>
              </label>
              <label className="tag-label">
                {sourceKind === "rss"
                  ? "Feed URL"
                  : sourceKind === "html_page"
                    ? "网页 URL"
                    : "图片 URL（空格或换行分隔）"}
                <textarea
                  rows={3}
                  value={sourceUrl}
                  onChange={(e) => setSourceUrl(e.target.value)}
                  placeholder={
                    sourceKind === "rss"
                      ? "https://example.com/feed.xml"
                      : sourceKind === "html_page"
                        ? "https://example.com/gallery"
                        : "https://…/a.jpg\nhttps://…/b.png"
                  }
                />
              </label>
              <div className="form-row">
                <label className="tag-label">
                  间隔（分钟）
                  <input
                    type="number"
                    min={5}
                    value={sourceInterval}
                    onChange={(e) => setSourceInterval(Number(e.target.value) || 360)}
                  />
                </label>
                <label className="tag-label">
                  入库文件夹
                  <select
                    value={folderId || ""}
                    onChange={(e) => setFolderId(e.target.value || null)}
                  >
                    <option value="">（未分类）</option>
                    {folders.map((f) => (
                      <option key={f.id} value={f.id}>
                        {f.name}
                      </option>
                    ))}
                  </select>
                </label>
              </div>
              <p className="hint">
                公开 RSS 或静态网页即可（遵守 robots.txt，每次最多 30 张）。登录墙站点请用浏览器插件「本页多选保存」，不做无人值守爬取。
              </p>
              <button type="button" className="primary" disabled={busy} onClick={onCreateSource}>
                添加并拉取
              </button>
              </div>
            {sources.length === 0 ? (
              <div className="empty">
                <p>还没有订阅源。</p>
              </div>
            ) : (
              sources.map((s) => (
                <div
                  key={s.id}
                  className={`source-card ${s.enabled ? "" : "dim"} ${sourceFilterId === s.id ? "source-card-active" : ""}`}
                  onClick={() => setSourceFilterId(s.id)}
                >
                  <div>
                    <div className="name">{s.name}</div>
                    <div className="hint">{s.kind} · {s.url}</div>
                    <div className="hint">
                      {s.lastRunAt
                        ? `上次 ${new Date(s.lastRunAt).toLocaleString()}`
                        : "尚未拉取"}
                      {s.lastError ? ` · ${s.lastError}` : ""}
                    </div>
                  </div>
                  <div className="source-actions">
                    <button type="button" className="primary" disabled={busy} onClick={(e) => { e.stopPropagation(); onPullSource(s.id); }}>
                      立即拉取
                    </button>
                    <button type="button" onClick={(e) => { e.stopPropagation(); onToggleSource(s); }}>
                      {s.enabled ? "暂停" : "启用"}
                    </button>
                    <button type="button" className="danger" onClick={(e) => { e.stopPropagation(); onDeleteSource(s); }}>
                      删除
                    </button>
                  </div>
                </div>
              ))
            )}
            </div>
            <div className="panel-right">
              <div className="result-head">
                <span>
                  结果
                  {sourceFilterId
                    ? ` · ${sources.find((s) => s.id === sourceFilterId)?.name ?? "当前源"}`
                    : " · 全部"}
                  {" "}
                  {assets.length} 张
                </span>
                {sourceFilterId ? (
                  <button type="button" className="linkish linkish-sm" onClick={() => setSourceFilterId(null)}>
                    看全部
                  </button>
                ) : null}
              </div>
              <section className="grid result-grid">
                {assets.length === 0 ? (
                  <div className="empty">
                    <p>
                      {tagFilter
                        ? `没有带「${tagFilter}」标签的订阅图片。`
                        : "还没有订阅图片。添加 RSS / 公开网页并拉取，或用浏览器插件保存后，会显示在这里。"}
                    </p>
                  </div>
                ) : (
                  assets.map((a) => (
                    <button
                      key={a.id}
                      type="button"
                      className={`card ${selected?.id === a.id ? "card-active" : ""}`}
                      onPointerDown={(e) => onLibCardPointerDown(a, e)}
                      onPointerMove={onLibCardPointerMove}
                      onPointerUp={() => onLibCardPointerUp(a)}
                      onPointerCancel={onLibCardPointerCancel}
                      title="单击详情 · 拖动可拖出"
                      style={{ borderColor: a.dominantColor || "transparent" }}
                    >
                      <RatioThumb
                        src={libraryRoot ? assetSrc(libraryRoot, a) : null}
                        alt={a.filename}
                        cacheKey={a.id}
                        width={a.width}
                        height={a.height}
                        background={a.dominantColor || undefined}
                      />
                      <div className="card-meta">
                        <span className="name">{a.filename}</span>
                        <span className="badge">{sourceKindLabel(a.sourceKind)}</span>
                      </div>
                    </button>
                  ))
                )}
              </section>
            </div>
          </section>
        ) : mode === "generate" ? (
          <section className="panel panel-split">
            <div className="panel-left">
            <div className="form-card">
              <label className="tag-label">
                描述
                <textarea
                  rows={4}
                  value={genPrompt}
                  onChange={(e) => setGenPrompt(e.target.value)}
                  placeholder="例如：暖色陶罐静物，侧光，胶片质感"
                />
              </label>
              <div className="form-row">
                <label className="tag-label">
                  张数
                  <input
                    type="number"
                    min={1}
                    max={8}
                    value={genN}
                    onChange={(e) => setGenN(Math.min(8, Math.max(1, Number(e.target.value) || 1)))}
                  />
                </label>
                <label className="tag-label">
                  尺寸
                  <select value={genSize} onChange={(e) => setGenSize(e.target.value)}>
                    <option value="512x512">512×512</option>
                    <option value="768x768">768×768</option>
                    <option value="1024x1024">1024×1024</option>
                    <option value="1280x720">1280×720（16:9）</option>
                  </select>
                </label>
                <label className="tag-label">
                  入库文件夹
                  <select
                    value={folderId || ""}
                    onChange={(e) => setFolderId(e.target.value || null)}
                  >
                    <option value="">（未分类）</option>
                    {folders.map((f) => (
                      <option key={f.id} value={f.id}>
                        {f.name}
                      </option>
                    ))}
                  </select>
                </label>
              </div>
              <p className="hint">
                {info?.imageGenProvider === "minimax"
                  ? "当前 MiniMax image-01。先生成预览，确认后再入库。"
                  : info?.imageGenProvider === "openai_compat"
                    ? "当前 OpenAI 兼容 Images API。先生成，再入库。"
                    : "当前 mock。先生成占位图，再入库。"}
              </p>
              <div className="form-actions">
                <button type="button" className="primary" disabled={busy} onClick={onGenerate}>
                  {busy ? "处理中…" : "生成"}
                </button>
                <button
                  type="button"
                  disabled={busy || !importableJob}
                  onClick={() => onImportGen()}
                >
                  入库
                </button>
              </div>
            </div>
            {genJobs.length === 0 ? (
              <div className="empty">
                <p>还没有生图任务。</p>
              </div>
            ) : (
              genJobs.map((j) => (
                <div
                  key={j.id}
                  className={`job-row ${genFilterId === j.id ? "source-card-active" : ""}`}
                  onClick={() => {
                    setGenFilterId(j.id);
                    setPickedGen(null);
                  }}
                >
                  <div>
                    <div className="name">{j.prompt}</div>
                    <div className="hint">
                      {jobStatusLabel(j.status)} · {j.provider}/{j.model} · n={j.n}
                      {j.status === "done" ? ` · 入库 ${j.imported}` : ""}
                      {j.status === "generated" ? ` · ${j.outputPaths?.length ?? 0} 张待入库` : ""}
                      {j.error ? ` · ${j.error}` : ""}
                    </div>
                  </div>
                  <div className="source-actions" onClick={(e) => e.stopPropagation()}>
                    {j.status === "generated" ? (
                      <button
                        type="button"
                        className="primary"
                        disabled={busy}
                        onClick={() => onImportGen(j.id)}
                      >
                        入库
                      </button>
                    ) : null}
                    <span className="badge">{jobStatusLabel(j.status)}</span>
                  </div>
                </div>
              ))
            )}
            </div>
            <div className="panel-right">
              <div className="result-head">
                <span>
                  {pickedGen ? "详情" : "结果"}
                  {pickedGen
                    ? ` · ${fileBaseName(pickedGen.path)}`
                    : genFilterId
                      ? " · 本次任务"
                      : " · 全部"}
                  {pickedGen
                    ? ""
                    : ` ${genJob?.status === "generated" ? `${genStaging.length} 张待入库` : `${assets.length} 张`}`}
                </span>
                {pickedGen ? (
                  <button type="button" className="linkish linkish-sm" onClick={() => setPickedGen(null)}>
                    返回结果
                  </button>
                ) : genFilterId ? (
                  <button type="button" className="linkish linkish-sm" onClick={() => setGenFilterId(null)}>
                    看全部
                  </button>
                ) : null}
              </div>
              {pickedGen ? (
                <div className="gen-detail">
                  {(pickedGenJob?.outputPaths?.length ?? 0) > 1 ? (
                    <div className="gen-strip">
                      {(pickedGenJob?.outputPaths ?? []).map((p) => (
                        <button
                          key={p}
                          type="button"
                          className={`card gen-strip-card ${pickedGen.path === p ? "card-active" : ""}`}
                          onClick={() => openGenDetail(p, pickedGen.jobId)}
                          title={fileBaseName(p)}
                        >
                          <RatioThumb src={convertFileSrc(p)} alt={fileBaseName(p)} cacheKey={`strip-${p}`} />
                        </button>
                      ))}
                    </div>
                  ) : null}
                  <div className="gen-detail-preview">
                    <img src={convertFileSrc(pickedGen.path)} alt={fileBaseName(pickedGen.path)} />
                  </div>
                  <h2>{fileBaseName(pickedGen.path)}</h2>
                  <dl>
                    <div>
                      <dt>描述</dt>
                      <dd>{pickedGenJob?.prompt || "—"}</dd>
                    </div>
                    <div>
                      <dt>尺寸</dt>
                      <dd>{pickedGenJob?.size || "—"}</dd>
                    </div>
                    <div>
                      <dt>模型</dt>
                      <dd>
                        {pickedGenJob
                          ? `${pickedGenJob.provider}/${pickedGenJob.model}`
                          : "—"}
                      </dd>
                    </div>
                    <div>
                      <dt>状态</dt>
                      <dd>待入库</dd>
                    </div>
                    <div>
                      <dt>路径</dt>
                      <dd className="path-dd">{pickedGen.path}</dd>
                    </div>
                  </dl>
                  <p className="hint">确认后再入库，写入素材库。同一任务会一并入库。</p>
                  <div className="detail-actions">
                    <button
                      type="button"
                      className="primary"
                      disabled={busy || !pickedGenJob}
                      onClick={() => onImportGen(pickedGen.jobId)}
                    >
                      {busy ? "处理中…" : "入库"}
                    </button>
                    <button type="button" onClick={() => onRevealGen(pickedGen.path)}>
                      Finder 显示
                    </button>
                  </div>
                </div>
              ) : (
              <section className="grid result-grid">
                {genStaging.length > 0 ? (
                  genStaging.map((p) => (
                    <button
                      key={p}
                      type="button"
                      className="card"
                      onClick={() => {
                        if (!genJob) return;
                        openGenDetail(p, genJob.id);
                      }}
                      title="单击查看详情"
                    >
                      <RatioThumb
                        src={convertFileSrc(p)}
                        alt={fileBaseName(p)}
                        cacheKey={p}
                      />
                      <div className="card-meta">
                        <span className="name">{fileBaseName(p)}</span>
                        <span className="badge">待入库</span>
                      </div>
                    </button>
                  ))
                ) : assets.length === 0 ? (
                      <div className="empty">
                        <p>
                          {tagFilter
                            ? `没有带「${tagFilter}」标签的生成图。`
                            : genJob?.status === "generated"
                              ? "生成文件丢失，请重新生成。"
                              : "先点「生成」预览，再点「入库」写入素材库。"}
                        </p>
                      </div>
                ) : (
                  assets.map((a) => (
                    <button
                      key={a.id}
                      type="button"
                      className={`card ${selected?.id === a.id ? "card-active" : ""}`}
                      onPointerDown={(e) => onLibCardPointerDown(a, e)}
                      onPointerMove={onLibCardPointerMove}
                      onPointerUp={() => onLibCardPointerUp(a)}
                      onPointerCancel={onLibCardPointerCancel}
                      title="单击详情 · 拖动可拖出"
                      style={{ borderColor: a.dominantColor || "transparent" }}
                    >
                      <RatioThumb
                        src={libraryRoot ? assetSrc(libraryRoot, a) : null}
                        alt={a.filename}
                        cacheKey={a.id}
                        width={a.width}
                        height={a.height}
                        background={a.dominantColor || undefined}
                      />
                      <div className="card-meta">
                        <span className="name">{a.filename}</span>
                        <span className="badge">{sourceKindLabel(a.sourceKind)}</span>
                      </div>
                    </button>
                  ))
                )}
              </section>
              )}
            </div>
          </section>
        ) : (
          <section className="grid">
            {assets.length === 0 ? (
              <div className="empty">
                <p>
                  {tagFilter
                    ? `没有带「${tagFilter}」标签的素材。点开一张图，再点标签旁 + 即可贴上。`
                    : "库是空的。订阅源和 AI 生图会保存在这里；外置卷、本地库、中转站只浏览原文件。"}
                </p>
                <p className="hint">外置卷 / 本地库 / 中转站不另存副本</p>
              </div>
            ) : (
              assets.map((a) => (
                <div
                  key={a.id}
                  className={`card ${selected?.id === a.id ? "card-active" : ""} ${libSelected.has(a.id) ? "card-selected" : ""}`}
                  style={{ borderColor: a.dominantColor || "transparent" }}
                >
                  <button
                    type="button"
                    className="card-check"
                    title="勾选 / 取消"
                    onClick={(e) => {
                      e.stopPropagation();
                      toggleLibSelect(a.id);
                    }}
                  >
                    {libSelected.has(a.id) ? "✓" : ""}
                  </button>
                  <button
                    type="button"
                    className="card-body"
                    onPointerDown={(e) => onLibCardPointerDown(a, e)}
                    onPointerMove={onLibCardPointerMove}
                    onPointerUp={() => onLibCardPointerUp(a)}
                    onPointerCancel={onLibCardPointerCancel}
                    title="单击详情 · 拖动可拖出到其他应用"
                  >
                    <RatioThumb
                      src={libraryRoot ? assetSrc(libraryRoot, a) : null}
                      alt={a.filename}
                      cacheKey={a.id}
                      width={a.width}
                      height={a.height}
                      background={a.dominantColor || undefined}
                    />
                    <div className="card-meta">
                      <span className="name">{a.filename}</span>
                      <span className="badge">
                        {a.sourceKind && a.sourceKind !== "local"
                          ? sourceKindLabel(a.sourceKind)
                          : a.clipStatus}
                      </span>
                    </div>
                  </button>
                </div>
              ))
            )}
          </section>
        )}

        {browseSelected.size > 0 || libSelected.size > 0 ? (
          <div className="selection-bar" role="status">
            <span className="selection-count">
              已选 {browseSelected.size + libSelected.size}
            </span>
            <button
              type="button"
              onClick={() => {
                setBrowseSelected(new Set());
                setLibSelected(new Set());
              }}
            >
              取消选择
            </button>
            <span className="selection-hint">
              点文件夹旁 + 可虚拟放入 · 点标签旁 + 可贴标签 · Esc 取消
            </span>
          </div>
        ) : null}
      </main>

      {detailOpen ? (
        <button
          type="button"
          className="detail-backdrop"
          aria-label="关闭详情"
          onClick={closeDetail}
        />
      ) : null}

      {picked ? (
        <aside className="detail">
          <button type="button" className="close" onClick={() => setPicked(null)}>
            关闭
          </button>
          <div className="detail-preview">
            <ExternalThumb item={picked} large />
          </div>
          <h2>{picked.name}</h2>
          <dl>
            <div>
              <dt>类型</dt>
              <dd>{picked.kind}</dd>
            </div>
            <div>
              <dt>大小</dt>
              <dd>{fmtSize(picked.fileSize)}</dd>
            </div>
            <div>
              <dt>路径</dt>
              <dd className="path-dd">{picked.path}</dd>
            </div>
          </dl>
          <div className="finder-tag-block">
            <div className="finder-tag-label">标签</div>
            <div className="finder-tag-row">
              {visibleFinderTags.map((t) => {
                const on = pickedTags.some((p) => p.name === t.name);
                return (
                  <button
                    key={t.name}
                    type="button"
                    className={`finder-tag-chip${on ? " on" : ""}`}
                    title={on ? `去掉「${t.name}」` : `贴上「${t.name}」`}
                    onClick={() => togglePickedTag(t, on)}
                  >
                    <span className="tag-dot" style={{ background: t.hex }} />
                    {t.name}
                  </button>
                );
              })}
              {pickedTags
                .filter((p) => !visibleFinderTags.some((t) => t.name === p.name))
                .map((p) => (
                  <button
                    key={p.name}
                    type="button"
                    className="finder-tag-chip on"
                    title={`去掉「${p.name}」`}
                    onClick={() =>
                      togglePickedTag(
                        {
                          name: p.name,
                          color: p.color,
                          hex: hexForFinderColor(p.color),
                        },
                        true,
                      )
                    }
                  >
                    <span
                      className="tag-dot"
                      style={{ background: hexForFinderColor(p.color) }}
                    />
                    {p.name}
                  </button>
                ))}
            </div>
            <div className="finder-tag-add">
              <input
                value={pickedTagDraft}
                onChange={(e) => setPickedTagDraft(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === "Enter") {
                    e.preventDefault();
                    addPickedTagByName();
                  }
                }}
                placeholder="新标签，回车贴上"
              />
              <button
                type="button"
                className="primary"
                disabled={!pickedTagDraft.trim()}
                onClick={() => addPickedTagByName()}
              >
                贴上
              </button>
            </div>
            <p className="hint">点色块切换；与 Finder 标签相同，不另存副本。</p>
          </div>
        </aside>
      ) : null}

      {selected ? (
        <aside className="detail">
          <button type="button" className="close" onClick={() => setSelected(null)}>
            关闭
          </button>
          <div className="detail-preview" style={{ background: selected.dominantColor || "#ddd" }}>
            {libraryRoot ? (
              <img src={assetSrc(libraryRoot, selected)} alt={selected.filename} />
            ) : null}
          </div>
          <h2>{selected.filename}</h2>
          <dl>
            <div>
              <dt>尺寸</dt>
              <dd>
                {selected.width && selected.height
                  ? `${selected.width}×${selected.height}`
                  : "—"}
              </dd>
            </div>
            <div>
              <dt>主色</dt>
              <dd>
                {selected.dominantColor ? (
                  <button
                    type="button"
                    className="color-chip"
                    style={{ background: selected.dominantColor }}
                    title="按此色搜图"
                    onClick={() => onColorSearch(selected.dominantColor!)}
                  >
                    {selected.dominantColor}
                  </button>
                ) : (
                  "—"
                )}
              </dd>
            </div>
            <div>
              <dt>来源</dt>
              <dd>{sourceKindLabel(selected.sourceKind)}</dd>
            </div>
            <div>
              <dt>CLIP</dt>
              <dd>{selected.clipStatus}</dd>
            </div>
            <div>
              <dt>入库</dt>
              <dd>{new Date(selected.createdAt).toLocaleString()}</dd>
            </div>
          </dl>

          <label className="tag-label">
            文件夹
            <select
              value={selected.folderId || ""}
              onChange={(e) => onMoveFolder(e.target.value || null)}
            >
              <option value="">（未分类）</option>
              {folders.map((f) => (
                <option key={f.id} value={f.id}>
                  {f.name}
                </option>
              ))}
            </select>
          </label>

          <label className="tag-label">
            标签（逗号分隔）
            <input value={tagDraft} onChange={(e) => setTagDraft(e.target.value)} />
          </label>
          <button type="button" className="primary" onClick={onSaveTags}>
            保存标签
          </button>

          <label className="tag-label">
            出处 URL
            <input value={sourceDraft} onChange={(e) => setSourceDraft(e.target.value)} />
          </label>
          <label className="tag-label">
            笔记
            <textarea
              rows={3}
              value={notesDraft}
              onChange={(e) => setNotesDraft(e.target.value)}
            />
          </label>
          <button type="button" onClick={onSaveMeta}>
            保存出处 / 笔记
          </button>

          <div className="detail-actions">
            <button
              type="button"
              className="primary"
              onClick={(e) => onDragAsset(selected, e)}
            >
              拖出到其他 App
            </button>
            <button type="button" onClick={onRevealAsset}>
              Finder 显示
            </button>
            <button type="button" className="danger" onClick={onDeleteAsset}>
              删除素材
            </button>
          </div>
        </aside>
      ) : null}
      {folderPicker && folderPickerCopy ? (
        <FolderPicker
          kind={folderPicker.kind}
          title={folderPickerCopy.title}
          confirmLabel={folderPickerCopy.confirmLabel}
          onPick={onPickFolder}
          onCancel={() => {
            setFolderPicker(null);
            setStatus("");
          }}
        />
      ) : null}
    </div>
  );
}
