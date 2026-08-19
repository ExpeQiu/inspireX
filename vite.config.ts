import { defineConfig, loadEnv } from "vite";
import react from "@vitejs/plugin-react";

// @ts-expect-error process is a nodejs global
const host = process.env.TAURI_DEV_HOST;

// https://vite.dev/config/
export default defineConfig(async ({ mode }) => {
  const env = loadEnv(mode, process.cwd(), "");
  const ingestPort = env.INSPIREX_INGEST_PORT || "18765";
  const ingestTarget = `http://127.0.0.1:${ingestPort}`;

  return {
    plugins: [react()],

    // Vite options tailored for Tauri development and only applied in `tauri dev` or `tauri build`
    //
    // 1. prevent Vite from obscuring rust errors
    clearScreen: false,
    // 2. tauri expects a fixed port, fail if that port is not available
    server: {
      port: 1420,
      strictPort: true,
      host: host || false,
      hmr: host
        ? {
            protocol: "ws",
            host,
            port: 1421,
          }
        : undefined,
      proxy: {
        "/v1": {
          target: ingestTarget,
          configure(proxy) {
            proxy.on("error", (err, _req, res) => {
              const out = res as {
                headersSent?: boolean;
                writeHead?: (code: number, headers: Record<string, string>) => void;
                end?: (body: string) => void;
              };
              if (out && !out.headersSent && typeof out.writeHead === "function") {
                out.writeHead(502, { "Content-Type": "application/json" });
                out.end?.(
                  JSON.stringify({
                    error: "ingest_unavailable",
                    detail: String(err),
                  }),
                );
              }
            });
          },
        },
        "/health": { target: ingestTarget },
      },
      watch: {
        // 3. tell Vite to ignore watching `src-tauri`
        ignored: ["**/src-tauri/**"],
      },
    },
  };
});
