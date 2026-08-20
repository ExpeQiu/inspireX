import {
  convertFileSrc as tauriConvertFileSrc,
  invoke as tauriInvoke,
} from "@tauri-apps/api/core";
import {
  httpSpec,
  ingestBasesFor,
  ingestMediaUrl,
  isIpcMissingMessage,
  isLocalHttpOrigin,
  browseThumbPath,
} from "./bridge-logic";

export { isIpcMissingMessage, browseThumbPath };

type IpcState = "unknown" | "yes" | "no";

let ipcState: IpcState = "unknown";
let ipcProbe: Promise<boolean> | null = null;

type TauriInternals = {
  invoke?: unknown;
  convertFileSrc?: unknown;
};

function internals(): TauriInternals | undefined {
  if (typeof window === "undefined") return undefined;
  return (window as unknown as { __TAURI_INTERNALS__?: TauriInternals })
    .__TAURI_INTERNALS__;
}

export function hasTauriIpc(): boolean {
  return typeof internals()?.invoke === "function";
}

function ingestPort(): string {
  return import.meta.env.VITE_INSPIREX_INGEST_PORT || "18765";
}

function pageOrigin(): string | undefined {
  if (typeof window === "undefined") return undefined;
  return window.location.origin;
}

function ingestBases(): string[] {
  return ingestBasesFor(pageOrigin(), ingestPort());
}

async function waitForTauri(ms: number): Promise<boolean> {
  if (hasTauriIpc()) return true;
  const t0 = Date.now();
  while (Date.now() - t0 < ms) {
    await new Promise((r) => setTimeout(r, 50));
    if (hasTauriIpc()) return true;
  }
  return hasTauriIpc();
}

export async function ensureIpc(): Promise<boolean> {
  if (hasTauriIpc()) {
    ipcState = "yes";
    return true;
  }
  if (ipcState === "yes") ipcState = "no";
  if (ipcState === "no") return false;
  if (!ipcProbe) {
    ipcProbe = (async () => {
      const ok = await waitForTauri(2000);
      ipcState = ok ? "yes" : "no";
      console.info("[inspirex]", {
        event: "ipc_probe",
        state: ipcState,
        origin: pageOrigin() ?? "",
        ms: 2000,
      });
      return ok;
    })();
  }
  return ipcProbe;
}

export function isIpcMissingError(e: unknown): boolean {
  const msg = e instanceof Error ? e.message : String(e);
  return isIpcMissingMessage(msg);
}

function sleep(ms: number): Promise<void> {
  return new Promise((r) => setTimeout(r, ms));
}

function isTransientFetchError(e: unknown): boolean {
  const msg = e instanceof Error ? e.message : String(e);
  return (
    /failed to fetch/i.test(msg) ||
    /networkerror/i.test(msg) ||
    /load failed/i.test(msg) ||
    /ingest_unavailable/i.test(msg) ||
    /econnrefused/i.test(msg)
  );
}

async function httpInvokeOnce<T>(
  base: string,
  cmd: string,
  args?: object,
): Promise<T> {
  const spec = httpSpec(cmd, args);
  const url = `${base}${spec.path}`;
  const res = await fetch(url, {
    method: spec.method,
    mode: "cors",
    credentials: "omit",
    cache: "no-store",
    headers: {
      Accept: "application/json",
      ...(spec.body ? { "Content-Type": "application/json" } : {}),
    },
    body: spec.body,
  });
  if (!res.ok) {
    let detail = res.statusText || `HTTP ${res.status}`;
    try {
      const body = (await res.json()) as { error?: string; detail?: string };
      if (body?.error) detail = body.error;
      else if (body?.detail) detail = body.detail;
    } catch {
      /* ignore */
    }
    throw new Error(detail);
  }
  return (await res.json()) as T;
}

async function httpInvoke<T>(cmd: string, args?: object): Promise<T> {
  const bases = ingestBases();
  const attempts = 6;
  const errors: string[] = [];
  for (let i = 0; i < attempts; i++) {
    if (i > 0 && hasTauriIpc()) {
      ipcState = "yes";
      ipcProbe = null;
      return tauriInvoke<T>(cmd, args as Record<string, unknown>);
    }
    errors.length = 0;
    for (const base of bases) {
      const label = base || "same-origin";
      try {
        const result = await httpInvokeOnce<T>(base, cmd, args);
        console.info("[inspirex]", {
          event: "ipc_http_fallback",
          cmd,
          base: label,
          attempt: i + 1,
        });
        return result;
      } catch (e) {
        errors.push(`${label}: ${e instanceof Error ? e.message : String(e)}`);
      }
    }
    const last = errors.join(" · ");
    if (i < attempts - 1 && errors.some((s) => isTransientFetchError(s))) {
      console.warn("[inspirex]", {
        event: "ipc_http_retry",
        cmd,
        attempt: i + 1,
        err: last,
      });
      await sleep(250 * (i + 1));
      continue;
    }
    break;
  }
  throw new Error(
    `桌面桥未就绪（${errors.join(" · ")}）。请用 ./scripts/start.sh 启动本机后端，浏览器打开 http://127.0.0.1:1420。`,
  );
}

export async function invoke<T>(cmd: string, args?: object): Promise<T> {
  if (hasTauriIpc()) ipcState = "yes";
  if (await ensureIpc()) {
    try {
      return await tauriInvoke<T>(cmd, args as Record<string, unknown>);
    } catch (e) {
      if (!isIpcMissingError(e)) throw e;
      ipcState = "no";
      console.warn("[inspirex]", { event: "ipc_invoke_missing", cmd, err: String(e) });
    }
  }
  return httpInvoke<T>(cmd, args);
}

export function convertFileSrc(filePath: string, protocol?: string): string {
  if (!filePath) return "";
  if (
    filePath.startsWith("http://") ||
    filePath.startsWith("https://") ||
    filePath.startsWith("asset:") ||
    filePath.startsWith("blob:")
  ) {
    return filePath;
  }
  if (ipcState === "yes" || hasTauriIpc()) {
    try {
      return tauriConvertFileSrc(filePath, protocol);
    } catch (e) {
      if (!isIpcMissingError(e)) throw e;
      ipcState = "no";
    }
  }
  const q = `path=${encodeURIComponent(filePath)}`;
  const origin = pageOrigin();
  if (origin && isLocalHttpOrigin(origin)) {
    return `/v1/browse/file?${q}`;
  }
  return `http://127.0.0.1:${ingestPort()}/v1/browse/file?${q}`;
}

/** 缩略图一次 GET 直接拿 JPEG，不再 RPC 取路径再读文件。 */
export function thumbSrc(filePath: string, maxSide = 240): string {
  if (!filePath) return "";
  return ingestMediaUrl(pageOrigin(), ingestPort(), browseThumbPath(filePath, maxSide));
}
