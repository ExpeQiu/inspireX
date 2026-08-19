#!/usr/bin/env python3
"""InspireX 本地 CLIP sidecar（stdin/stdout JSON Lines）。

协议：
  请求：{"op":"embed_image","path":"/abs/file.jpg"}
        {"op":"embed_text","text":"a red chair"}
        {"op":"ping"}
  响应：{"ok":true,"vector":[...],"dim":512,"model":"openai/clip-vit-base-patch32"}
        {"ok":false,"error":"..."}

依赖：torch + transformers（已装则可直接用）。
模型默认：openai/clip-vit-base-patch32（可用 CLIP_MODEL 覆盖）。
"""

from __future__ import annotations

import json
import os
import sys
import traceback


MODEL_ID = os.environ.get("CLIP_MODEL", "openai/clip-vit-base-patch32")


def log(msg: str) -> None:
    print(msg, file=sys.stderr, flush=True)


def load_model():
    import torch
    from transformers import CLIPModel, CLIPProcessor

    device = "mps" if torch.backends.mps.is_available() else "cpu"
    log(f"clip_sidecar loading model={MODEL_ID} device={device}")
    processor = CLIPProcessor.from_pretrained(MODEL_ID)
    model = CLIPModel.from_pretrained(MODEL_ID).to(device)
    model.eval()
    return processor, model, device


def embed_image(processor, model, device, path: str):
    import torch
    from PIL import Image

    image = Image.open(path).convert("RGB")
    inputs = processor(images=image, return_tensors="pt")
    inputs = {k: v.to(device) for k, v in inputs.items()}
    with torch.no_grad():
        feats = model.get_image_features(**inputs)
        feats = feats / feats.norm(dim=-1, keepdim=True)
    return feats[0].detach().cpu().tolist()


def embed_text(processor, model, device, text: str):
    import torch

    inputs = processor(text=[text], return_tensors="pt", padding=True, truncation=True)
    inputs = {k: v.to(device) for k, v in inputs.items()}
    with torch.no_grad():
        feats = model.get_text_features(**inputs)
        feats = feats / feats.norm(dim=-1, keepdim=True)
    return feats[0].detach().cpu().tolist()


def main() -> int:
    try:
        processor, model, device = load_model()
    except Exception as e:
        log(f"clip_sidecar_init_fail: {e}")
        # 仍可读 stdin，对每条请求返回错误，便于 Rust 侧降级
        processor = model = device = None
        init_err = str(e)
    else:
        init_err = None
        log("clip_sidecar ready")

    for line in sys.stdin:
        line = line.strip()
        if not line:
            continue
        try:
            req = json.loads(line)
            op = req.get("op")
            if op == "ping":
                if init_err:
                    print(json.dumps({"ok": False, "error": init_err}), flush=True)
                else:
                    print(
                        json.dumps({"ok": True, "model": MODEL_ID, "device": str(device)}),
                        flush=True,
                    )
                continue
            if init_err or processor is None:
                print(json.dumps({"ok": False, "error": init_err or "model not loaded"}), flush=True)
                continue
            if op == "embed_image":
                path = req.get("path") or ""
                vec = embed_image(processor, model, device, path)
                print(
                    json.dumps(
                        {"ok": True, "vector": vec, "dim": len(vec), "model": MODEL_ID}
                    ),
                    flush=True,
                )
            elif op == "embed_text":
                text = req.get("text") or ""
                vec = embed_text(processor, model, device, text)
                print(
                    json.dumps(
                        {"ok": True, "vector": vec, "dim": len(vec), "model": MODEL_ID}
                    ),
                    flush=True,
                )
            else:
                print(json.dumps({"ok": False, "error": f"unknown op: {op}"}), flush=True)
        except Exception as e:
            log(traceback.format_exc())
            print(json.dumps({"ok": False, "error": str(e)}), flush=True)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
