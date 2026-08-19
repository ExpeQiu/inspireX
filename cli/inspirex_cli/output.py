"""人类可读 table / 机器可读 json / csv / markdown。数据走 stdout。"""

from __future__ import annotations

import csv
import io
import json
from pathlib import Path
from typing import Any, Optional

import click

from inspirex_cli.models import CliEnvelope

FORMATS = ("table", "json", "csv", "markdown")


def render(envelope: CliEnvelope, fmt: str) -> str:
    if fmt == "json":
        return json.dumps(envelope.model_dump(exclude_none=True), ensure_ascii=False, indent=2)
    rows = _rows(envelope)
    if fmt == "csv":
        return _to_csv(rows)
    if fmt == "markdown":
        return _to_markdown(rows)
    return _to_table(rows, envelope)


def emit(envelope: CliEnvelope, fmt: str, output: Optional[str]) -> None:
    text = render(envelope, fmt)
    if output:
        Path(output).write_text(text + ("" if text.endswith("\n") else "\n"), encoding="utf-8")
        click.echo(f"已写入 {output}", err=True)
    else:
        click.echo(text)


def _rows(envelope: CliEnvelope) -> list[dict[str, Any]]:
    if envelope.payload and not envelope.items:
        return [envelope.payload]
    items = envelope.items or []
    out: list[dict[str, Any]] = []
    for item in items:
        if isinstance(item, dict):
            out.append(item)
        else:
            out.append({"value": item})
    return out


def _flatten(row: dict[str, Any]) -> dict[str, str]:
    flat: dict[str, str] = {}
    for k, v in row.items():
        if isinstance(v, (list, dict)):
            flat[k] = json.dumps(v, ensure_ascii=False)
        elif v is None:
            flat[k] = ""
        else:
            flat[k] = str(v)
    return flat


def _headers(rows: list[dict[str, Any]]) -> list[str]:
    preferred = [
        "id",
        "filename",
        "name",
        "kind",
        "path",
        "libraryPath",
        "assetCount",
        "ok",
        "imported",
        "skipped",
        "tags",
        "sourceUrl",
    ]
    keys: list[str] = []
    seen: set[str] = set()
    for row in rows:
        for k in row:
            if k not in seen:
                seen.add(k)
                keys.append(k)
    ordered = [k for k in preferred if k in seen]
    rest = [k for k in keys if k not in ordered]
    return ordered + rest


def _to_csv(rows: list[dict[str, Any]]) -> str:
    if not rows:
        return ""
    flats = [_flatten(r) for r in rows]
    headers = _headers(rows)
    buf = io.StringIO()
    writer = csv.DictWriter(buf, fieldnames=headers, extrasaction="ignore")
    writer.writeheader()
    for row in flats:
        writer.writerow({h: row.get(h, "") for h in headers})
    return buf.getvalue().rstrip("\n")


def _to_markdown(rows: list[dict[str, Any]]) -> str:
    if not rows:
        return "_（空）_"
    headers = _headers(rows)
    flats = [_flatten(r) for r in rows]
    lines = [
        "| " + " | ".join(headers) + " |",
        "| " + " | ".join("---" for _ in headers) + " |",
    ]
    for row in flats:
        lines.append("| " + " | ".join(row.get(h, "") for h in headers) + " |")
    return "\n".join(lines)


def _to_table(rows: list[dict[str, Any]], envelope: CliEnvelope) -> str:
    head = f"{envelope.module}  source={envelope.data_source}  count={envelope.count}"
    if envelope.warnings:
        head += f"  warnings={len(envelope.warnings)}"
    if not rows:
        return head + "\n（空）"
    headers = _headers(rows)
    # 表格列过多时只展示关键列
    show = headers[:8]
    flats = [_flatten(r) for r in rows]
    widths = {h: len(h) for h in show}
    for row in flats:
        for h in show:
            widths[h] = max(widths[h], min(len(row.get(h, "")), 40))
    def cell(h: str, val: str) -> str:
        if len(val) > widths[h]:
            val = val[: widths[h] - 1] + "…"
        return val.ljust(widths[h])
    lines = [head, "  ".join(cell(h, h) for h in show)]
    lines.append("  ".join("-" * widths[h] for h in show))
    for row in flats:
        lines.append("  ".join(cell(h, row.get(h, "")) for h in show))
    return "\n".join(lines)
