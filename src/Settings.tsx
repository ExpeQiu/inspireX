import { useEffect, useState } from "react";
import { invoke } from "./bridge";

export type AiEndpointView = {
  provider: string;
  baseUrl: string;
  model: string;
  apiKeySet: boolean;
  effective: string;
};

export type AiServicesView = {
  text: AiEndpointView;
  image: AiEndpointView;
  video: AiEndpointView;
  configPath: string;
};

type Kind = "text" | "image" | "video";

type Draft = {
  provider: string;
  baseUrl: string;
  model: string;
  apiKey: string;
};

const KINDS: { id: Kind; title: string; hint: string }[] = [
  { id: "text", title: "文本", hint: "提示词挖掘、视觉理解。MiniMax 用 MiniMax-M3。无 Key 时走 mock。" },
  { id: "image", title: "图片", hint: "文生图。MiniMax 用 image-01（M3 是语言模型，不能出图）。无 Key 时生成占位图。" },
  { id: "video", title: "视频", hint: "配置先保存；视频生成接入后使用此处服务。" },
];

function emptyDraft(): Draft {
  return { provider: "mock", baseUrl: "", model: "", apiKey: "" };
}

function fromView(v: AiEndpointView | undefined): Draft {
  return {
    provider: v?.provider || "mock",
    baseUrl: v?.baseUrl || "",
    model: v?.model || "",
    apiKey: "",
  };
}

function effectiveLabel(v: AiEndpointView | undefined, draft: Draft): string {
  const provider = draft.provider || v?.provider || "mock";
  const hasKey = Boolean(draft.apiKey.trim()) || Boolean(v?.apiKeySet);
  if (provider === "mock" || !hasKey) return "mock";
  return provider;
}

export function SettingsPanel({ onSaved }: { onSaved?: () => void }) {
  const [view, setView] = useState<AiServicesView | null>(null);
  const [text, setText] = useState<Draft>(emptyDraft);
  const [image, setImage] = useState<Draft>(emptyDraft);
  const [video, setVideo] = useState<Draft>(emptyDraft);
  const [busy, setBusy] = useState(false);
  const [status, setStatus] = useState("");

  const drafts: Record<Kind, Draft> = { text, image, video };
  const setters: Record<Kind, (d: Draft) => void> = {
    text: setText,
    image: setImage,
    video: setVideo,
  };

  useEffect(() => {
    let cancelled = false;
    invoke<AiServicesView>("get_ai_settings")
      .then((got) => {
        if (cancelled) return;
        setView(got);
        setText(fromView(got.text));
        setImage(fromView(got.image));
        setVideo(fromView(got.video));
        console.info("[inspirex]", {
          event: "ai_settings_load",
          text: got.text.effective,
          image: got.image.effective,
          video: got.video.effective,
        });
      })
      .catch((e) => {
        if (!cancelled) setStatus(String(e));
      });
    return () => {
      cancelled = true;
    };
  }, []);

  const save = async () => {
    setBusy(true);
    setStatus("保存中…");
    try {
      const patchOf = (kind: Kind) => {
        const d = drafts[kind];
        return {
          provider: d.provider,
          baseUrl: d.baseUrl,
          model: d.model,
          apiKey: d.apiKey,
        };
      };
      const got = await invoke<AiServicesView>("set_ai_settings", {
        text: patchOf("text"),
        image: patchOf("image"),
        video: patchOf("video"),
      });
      setView(got);
      setText(fromView(got.text));
      setImage(fromView(got.image));
      setVideo(fromView(got.video));
      setStatus("已保存");
      console.info("[inspirex]", {
        event: "ai_settings_save",
        text: got.text.effective,
        image: got.image.effective,
        video: got.video.effective,
      });
      onSaved?.();
    } catch (e) {
      setStatus(String(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <section className="panel">
      <div className="settings-grid">
        {KINDS.map((k) => {
          const d = drafts[k.id];
          const v = view?.[k.id];
          return (
            <div key={k.id} className="form-card settings-card">
              <h3>{k.title}</h3>
              <p className="hint">{k.hint}</p>
              <label className="tag-label">
                服务商
                <select
                  value={d.provider}
                  onChange={(e) => setters[k.id]({ ...d, provider: e.target.value })}
                >
                  <option value="mock">mock（本地占位）</option>
                  <option value="minimax">MiniMax</option>
                  <option value="openai_compat">OpenAI 兼容</option>
                </select>
              </label>
              <label className="tag-label">
                Base URL
                <input
                  value={d.baseUrl}
                  onChange={(e) => setters[k.id]({ ...d, baseUrl: e.target.value })}
                  placeholder={d.provider === "minimax" ? "https://api.minimaxi.com" : "https://api.openai.com/v1"}
                  disabled={d.provider === "mock"}
                />
              </label>
              <label className="tag-label">
                模型
                <input
                  value={d.model}
                  onChange={(e) => setters[k.id]({ ...d, model: e.target.value })}
                  placeholder={
                    k.id === "image"
                      ? "image-01"
                      : k.id === "text"
                        ? "MiniMax-M3"
                        : "MiniMax-Hailuo-02"
                  }
                  disabled={d.provider === "mock"}
                />
              </label>
              <label className="tag-label">
                API Key
                <input
                  type="password"
                  autoComplete="off"
                  value={d.apiKey}
                  onChange={(e) => setters[k.id]({ ...d, apiKey: e.target.value })}
                  placeholder={v?.apiKeySet ? "已保存，留空则保持" : "sk-…"}
                  disabled={d.provider === "mock"}
                />
              </label>
              <p className="settings-effective">
                实际生效：{effectiveLabel(v, d)}
                {d.provider !== "mock" && !d.apiKey.trim() && !v?.apiKeySet ? "（未填 Key，回退 mock）" : ""}
              </p>
            </div>
          );
        })}
        <div className="form-actions">
          <button type="button" className="primary" disabled={busy} onClick={() => void save()}>
            {busy ? "保存中…" : "保存"}
          </button>
          {status ? <span className="settings-status">{status}</span> : null}
        </div>
        {view?.configPath ? (
          <p className="hint">配置文件：{view.configPath}</p>
        ) : null}
      </div>
    </section>
  );
}
