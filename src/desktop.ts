import { open as tauriOpen } from "@tauri-apps/plugin-dialog";
import { ensureIpc, isIpcMissingError } from "./bridge";

export type PathPick =
  | { ok: true; paths: string[] }
  | { ok: false; reason: "cancel" | "need-path" };

function asList(chosen: string | string[] | null): string[] {
  if (!chosen) return [];
  return Array.isArray(chosen) ? chosen.filter(Boolean) : [chosen];
}

export async function pickDirectory(): Promise<PathPick> {
  if (await ensureIpc()) {
    try {
      const chosen = await tauriOpen({ directory: true, multiple: false });
      const paths = asList(chosen);
      if (paths.length === 0) return { ok: false, reason: "cancel" };
      console.info("[inspirex]", { event: "pick_directory", via: "tauri" });
      return { ok: true, paths };
    } catch (e) {
      if (!isIpcMissingError(e)) throw e;
      console.warn("[inspirex]", { event: "pick_directory_ipc_missing", err: String(e) });
    }
  }
  return { ok: false, reason: "need-path" };
}

export async function pickFiles(extensions: string[]): Promise<PathPick> {
  if (await ensureIpc()) {
    try {
      const chosen = await tauriOpen({
        multiple: true,
        directory: false,
        filters: [{ name: "Media", extensions }],
      });
      const paths = asList(chosen);
      if (paths.length === 0) return { ok: false, reason: "cancel" };
      console.info("[inspirex]", { event: "pick_files", via: "tauri", n: paths.length });
      return { ok: true, paths };
    } catch (e) {
      if (!isIpcMissingError(e)) throw e;
      console.warn("[inspirex]", { event: "pick_files_ipc_missing", err: String(e) });
    }
  }
  return { ok: false, reason: "need-path" };
}

export function splitPathList(raw: string): string[] {
  return raw
    .split(/[\n,]+/)
    .map((s) => s.trim())
    .filter(Boolean);
}
