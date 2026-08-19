import { useEffect, useMemo, useState } from "react";
import { invoke } from "./bridge";

type ExternalItem = {
  path: string;
  name: string;
  kind: string;
};

type ExternalListing = {
  root: string;
  path: string;
  parent?: string | null;
  folders: ExternalItem[];
};

export type FolderPickerKind = "local" | "volumes";

type Props = {
  kind: FolderPickerKind;
  title: string;
  confirmLabel: string;
  onPick: (path: string) => void;
  onCancel: () => void;
};

function isPhotosLibrary(path: string): boolean {
  return /\.photoslibrary$/i.test(path.replace(/\/$/, ""));
}

function shortcutsFor(kind: FolderPickerKind, root: string): { name: string; path: string }[] {
  if (kind === "volumes") {
    return [{ name: "外置卷", path: "/Volumes" }];
  }
  const home = root.replace(/\/$/, "");
  return [
    { name: "家目录", path: home },
    { name: "桌面", path: `${home}/Desktop` },
    { name: "图片", path: `${home}/Pictures` },
    { name: "影片", path: `${home}/Movies` },
    { name: "下载", path: `${home}/Downloads` },
  ];
}

function folderLabel(path: string, fallback: string): string {
  if (isPhotosLibrary(path)) {
    const name = path.replace(/\/$/, "").split("/").pop() || fallback;
    if (/^Photos Library\.photoslibrary$/i.test(name)) return "照片";
    return name.replace(/\.photoslibrary$/i, "");
  }
  const parts = path.replace(/\/$/, "").split("/").filter(Boolean);
  return parts[parts.length - 1] || fallback;
}

export function FolderPicker({ kind, title, confirmLabel, onPick, onCancel }: Props) {
  const [navPath, setNavPath] = useState<string | null>(null);
  const [listing, setListing] = useState<ExternalListing | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(true);

  useEffect(() => {
    let cancelled = false;
    (async () => {
      setBusy(true);
      setError(null);
      try {
        const next = await invoke<ExternalListing>("list_picker_dir", {
          path: navPath,
          kind,
        });
        if (cancelled) return;
        setListing(next);
        console.info("[inspirex]", {
          event: "folder_picker_list",
          kind,
          path: next.path,
          folders: next.folders.length,
        });
      } catch (e) {
        if (cancelled) return;
        setError(e instanceof Error ? e.message : String(e));
      } finally {
        if (!cancelled) setBusy(false);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [kind, navPath]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onCancel();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onCancel]);

  const shortcuts = useMemo(
    () => (listing ? shortcutsFor(kind, listing.root) : []),
    [kind, listing],
  );

  const atVolumesRoot =
    kind === "volumes" &&
    !!listing &&
    (listing.path === listing.root || listing.path === "/Volumes");
  const canConfirm = Boolean(listing?.path) && !atVolumesRoot && !busy;
  const currentName = listing ? folderLabel(listing.path, "此文件夹") : "";

  return (
    <div className="folder-picker-backdrop" onClick={onCancel} role="presentation">
      <div
        className="folder-picker"
        role="dialog"
        aria-modal="true"
        aria-labelledby="folder-picker-title"
        onClick={(e) => e.stopPropagation()}
      >
        <header className="folder-picker-head">
          <h2 id="folder-picker-title">{title}</h2>
          <p className="folder-picker-path">{listing?.path || "正在打开…"}</p>
        </header>
        {shortcuts.length > 0 ? (
          <div className="folder-picker-shortcuts">
            {shortcuts.map((s) => (
              <button
                key={s.path}
                type="button"
                className={listing?.path === s.path ? "active" : ""}
                onClick={() => setNavPath(s.path)}
              >
                {s.name}
              </button>
            ))}
          </div>
        ) : null}
        <div className="folder-picker-list">
          {listing?.parent ? (
            <button
              type="button"
              className="folder-picker-item folder-picker-up"
              onClick={() => setNavPath(listing.parent || listing.root)}
            >
              <span className="folder-picker-icon">↑</span>
              <span>上一级</span>
            </button>
          ) : null}
          {busy && !listing ? <p className="folder-picker-hint">正在列出文件夹…</p> : null}
          {error ? <p className="folder-picker-error">{error}</p> : null}
          {!busy && listing && listing.folders.length === 0 && !error ? (
            <p className="folder-picker-hint">
              {isPhotosLibrary(listing.path)
                ? "这是照片图库，点下方按钮即可绑定。"
                : "此目录下没有子文件夹，可直接选择当前目录。"}
            </p>
          ) : null}
          {listing?.folders.map((f) => (
            <button
              key={f.path}
              type="button"
              className="folder-picker-item"
              onClick={() => setNavPath(f.path)}
            >
              <span className="folder-picker-icon">📁</span>
              <span className="folder-picker-name">{f.name}</span>
            </button>
          ))}
        </div>
        <footer className="folder-picker-foot">
          <button type="button" onClick={onCancel}>
            取消
          </button>
          <button
            type="button"
            className="primary"
            disabled={!canConfirm}
            onClick={() => {
              if (!listing?.path || !canConfirm) return;
              console.info("[inspirex]", {
                event: "folder_picker_confirm",
                kind,
                path: listing.path,
              });
              onPick(listing.path);
            }}
          >
            {confirmLabel}
            {currentName ? `「${currentName}」` : ""}
          </button>
        </footer>
      </div>
    </div>
  );
}
