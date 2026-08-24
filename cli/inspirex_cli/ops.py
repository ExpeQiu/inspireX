"""命令实现：demo 夹具或 live RPC。"""

from __future__ import annotations

from typing import Any, Optional
from uuid import uuid4

from inspirex_cli import demo
from inspirex_cli.client import CliClientError, InspirexClient
from inspirex_cli.config import CliConfig, config_summary
from inspirex_cli.models import CliEnvelope
from inspirex_cli.utils.errors import EXIT_ERROR, EXIT_NO_DATA, EXIT_OK, EXIT_SCRAPE_FAIL
from inspirex_cli.utils.logger import setup_logger

logger = setup_logger()


def _run_id() -> str:
    return uuid4().hex[:8]


def _as_list(value: Any) -> list[Any]:
    if value is None:
        return []
    if isinstance(value, list):
        return value
    return [value]


class CommandResult:
    def __init__(self, envelope: CliEnvelope, exit_code: int = EXIT_OK):
        self.envelope = envelope
        self.exit_code = exit_code


def status(cfg: CliConfig, *, demo_mode: bool) -> CommandResult:
    rid = _run_id()
    logger.info("cli_status run_id=%s demo=%s", rid, demo_mode)
    if demo_mode:
        health = demo.demo_health()
        info = demo.demo_library_info()
        env = CliEnvelope(module="status", data_source="demo", payload={**health, **info})
        env.with_items([{**health, **info}])
        return CommandResult(env)
    client = InspirexClient(cfg)
    try:
        health = client.health()
        info = client.invoke("get_library_info", {})
    except CliClientError as e:
        env = CliEnvelope(module="status", data_source="live", warnings=[str(e)])
        env.with_items([])
        return CommandResult(env, e.exit_code)
    merged = {**(health if isinstance(health, dict) else {}), **(info if isinstance(info, dict) else {})}
    env = CliEnvelope(module="status", data_source="live", payload=merged)
    env.with_items([merged])
    return CommandResult(env)


def config_check(cfg: CliConfig, *, demo_mode: bool) -> CommandResult:
    rid = _run_id()
    logger.info("cli_config_check run_id=%s demo=%s origin=%s", rid, demo_mode, cfg.origin)
    backend_ok: Optional[bool]
    warnings: list[str] = []
    if demo_mode:
        backend_ok = True
    else:
        try:
            InspirexClient(cfg).health()
            backend_ok = True
        except CliClientError as e:
            backend_ok = False
            warnings.append(str(e))
    summary = config_summary(cfg, backend_ok=backend_ok)
    env = CliEnvelope(
        module="config-check",
        data_source="demo" if demo_mode else "live",
        warnings=warnings,
        payload=summary,
    )
    env.with_items([summary])
    return CommandResult(env)


def library_info(cfg: CliConfig, *, demo_mode: bool) -> CommandResult:
    rid = _run_id()
    logger.info("cli_library_info run_id=%s demo=%s", rid, demo_mode)
    if demo_mode:
        info = demo.demo_library_info()
        env = CliEnvelope(module="library-info", data_source="demo", payload=info)
        env.with_items([info])
        return CommandResult(env)
    return _invoke_one(cfg, "library-info", "get_library_info", {})


def asset_list(
    cfg: CliConfig,
    *,
    demo_mode: bool,
    folder_id: Optional[str] = None,
    limit: int = 50,
) -> CommandResult:
    rid = _run_id()
    logger.info("cli_asset_list run_id=%s demo=%s folder_id=%s limit=%s", rid, demo_mode, folder_id, limit)
    if demo_mode:
        items = demo.demo_assets()[:limit]
        env = CliEnvelope(module="asset-list", data_source="demo")
        env.with_items(items)
        return CommandResult(env)
    return _invoke_list(
        cfg,
        "asset-list",
        "list_assets",
        {"folderId": folder_id, "limit": limit},
        empty_is_ok=True,
    )


def asset_search(
    cfg: CliConfig,
    *,
    demo_mode: bool,
    text: str = "",
    tag: Optional[str] = None,
    color: Optional[str] = None,
    clip: bool = False,
    limit: int = 20,
) -> CommandResult:
    rid = _run_id()
    logger.info(
        "cli_asset_search run_id=%s demo=%s text=%s tag=%s clip=%s",
        rid,
        demo_mode,
        text,
        tag,
        clip,
    )
    if demo_mode:
        items = demo.demo_assets()
        q = (text or tag or color or "").lower()
        if q:
            items = [
                a
                for a in items
                if q in a["filename"].lower()
                or q in " ".join(a.get("tags") or []).lower()
                or q in (a.get("dominantColor") or "").lower()
            ]
        env = CliEnvelope(module="asset-search", data_source="demo")
        env.with_items(items[:limit])
        return CommandResult(env, EXIT_OK if items else EXIT_NO_DATA)
    query: dict[str, Any] = {
        "text": text or None,
        "tag": tag,
        "colorHex": color,
        "useClip": clip,
        "limit": limit,
    }
    return _invoke_list(cfg, "asset-search", "search_assets", {"query": query}, empty_is_ok=False)


def asset_import(
    cfg: CliConfig,
    *,
    demo_mode: bool,
    paths: list[str],
    folder_id: Optional[str] = None,
    dry_run: bool = False,
) -> CommandResult:
    rid = _run_id()
    logger.info(
        "cli_asset_import run_id=%s demo=%s n=%s dry_run=%s",
        rid,
        demo_mode,
        len(paths),
        dry_run or demo_mode,
    )
    if not paths:
        env = CliEnvelope(module="asset-import", data_source="demo" if demo_mode else "live")
        env.warnings.append("未指定路径")
        return CommandResult(env, EXIT_ERROR)
    if demo_mode or dry_run:
        payload = demo.demo_import_result(list(paths))
        if dry_run and not demo_mode:
            payload["dryRun"] = True
        env = CliEnvelope(
            module="asset-import",
            data_source="demo" if demo_mode else "live",
            payload=payload,
        )
        env.with_items([payload])
        return CommandResult(env)
    try:
        data = InspirexClient(cfg).invoke(
            "import_files",
            {"paths": list(paths), "folderId": folder_id},
        )
    except CliClientError as e:
        env = CliEnvelope(module="asset-import", data_source="live", warnings=[str(e)])
        return CommandResult(env, e.exit_code)
    payload = data if isinstance(data, dict) else {"result": data}
    env = CliEnvelope(module="asset-import", data_source="live", payload=payload)
    env.with_items([payload])
    errors = payload.get("errors") or []
    imported = int(payload.get("imported") or 0)
    if errors and imported == 0:
        return CommandResult(env, EXIT_SCRAPE_FAIL)
    return CommandResult(env)


def asset_import_url(
    cfg: CliConfig,
    *,
    demo_mode: bool,
    image_url: str,
    page_url: Optional[str] = None,
    title: Optional[str] = None,
    folder_id: Optional[str] = None,
) -> CommandResult:
    rid = _run_id()
    logger.info("cli_asset_import_url run_id=%s demo=%s", rid, demo_mode)
    if demo_mode:
        payload = demo.demo_import_result([image_url])
        payload["imageUrl"] = image_url
        env = CliEnvelope(module="asset-import-url", data_source="demo", payload=payload)
        env.with_items([payload])
        return CommandResult(env)
    try:
        data = InspirexClient(cfg).invoke(
            "import_url",
            {
                "req": {
                    "imageUrl": image_url,
                    "pageUrl": page_url,
                    "title": title,
                    "folderId": folder_id,
                }
            },
        )
    except CliClientError as e:
        env = CliEnvelope(module="asset-import-url", data_source="live", warnings=[str(e)])
        return CommandResult(env, e.exit_code)
    payload = data if isinstance(data, dict) else {"result": data}
    env = CliEnvelope(module="asset-import-url", data_source="live", payload=payload)
    env.with_items([payload])
    return CommandResult(env)


def folder_list(cfg: CliConfig, *, demo_mode: bool) -> CommandResult:
    if demo_mode:
        env = CliEnvelope(module="folder-list", data_source="demo")
        env.with_items(demo.demo_folders())
        return CommandResult(env)
    return _invoke_list(cfg, "folder-list", "list_folders", {}, empty_is_ok=True)


def folder_create(cfg: CliConfig, *, demo_mode: bool, name: str, parent_id: Optional[str] = None) -> CommandResult:
    logger.info("cli_folder_create name=%s demo=%s", name, demo_mode)
    if demo_mode:
        item = {"id": "demo-folder-new", "name": name, "parentId": parent_id, "itemCount": 0}
        env = CliEnvelope(module="folder-create", data_source="demo")
        env.with_items([item])
        return CommandResult(env)
    return _invoke_one(cfg, "folder-create", "create_folder", {"name": name, "parentId": parent_id})


def tag_list(cfg: CliConfig, *, demo_mode: bool) -> CommandResult:
    if demo_mode:
        env = CliEnvelope(module="tag-list", data_source="demo")
        env.with_items(demo.demo_tags())
        return CommandResult(env)
    return _invoke_list(cfg, "tag-list", "list_tags", {}, empty_is_ok=True)


def tag_create(cfg: CliConfig, *, demo_mode: bool, name: str, color: Optional[str] = None) -> CommandResult:
    logger.info("cli_tag_create name=%s demo=%s", name, demo_mode)
    if demo_mode:
        item = {"id": "demo-tag-new", "name": name, "color": color}
        env = CliEnvelope(module="tag-create", data_source="demo")
        env.with_items([item])
        return CommandResult(env)
    return _invoke_one(cfg, "tag-create", "create_tag", {"name": name, "color": color})


def source_list(cfg: CliConfig, *, demo_mode: bool) -> CommandResult:
    if demo_mode:
        env = CliEnvelope(module="source-list", data_source="demo")
        env.with_items(demo.demo_sources())
        return CommandResult(env)
    return _invoke_list(cfg, "source-list", "list_sources", {}, empty_is_ok=True)


def source_pull(cfg: CliConfig, *, demo_mode: bool, source_id: str) -> CommandResult:
    logger.info("cli_source_pull id=%s demo=%s", source_id, demo_mode)
    if demo_mode:
        payload = demo.demo_import_result([f"source:{source_id}"])
        env = CliEnvelope(module="source-pull", data_source="demo", payload=payload)
        env.with_items([payload])
        return CommandResult(env)
    return _invoke_one(cfg, "source-pull", "pull_source", {"sourceId": source_id}, scrape=True)


def hub_status(cfg: CliConfig, *, demo_mode: bool) -> CommandResult:
    if demo_mode:
        info = demo.demo_hub_status()
        env = CliEnvelope(module="hub-status", data_source="demo", payload=info)
        env.with_items([info])
        return CommandResult(env)
    return _invoke_one(cfg, "hub-status", "get_hub_status", {})


def prompt_status(cfg: CliConfig, *, demo_mode: bool) -> CommandResult:
    if demo_mode:
        info = demo.demo_vprompt_status()
        env = CliEnvelope(module="prompt-status", data_source="demo", payload=info)
        env.with_items([info])
        return CommandResult(env)
    return _invoke_one(cfg, "prompt-status", "get_vprompt_status", {})


def prompt_sync(cfg: CliConfig, *, demo_mode: bool, root: str | None = None) -> CommandResult:
    logger.info("cli_prompt_sync demo=%s root=%s", demo_mode, root)
    if demo_mode:
        info = demo.demo_vprompt_sync()
        env = CliEnvelope(module="prompt-sync", data_source="demo", payload=info)
        env.with_items([info])
        return CommandResult(env)
    args: dict[str, Any] = {}
    if root:
        args["root"] = root
    return _invoke_one(cfg, "prompt-sync", "sync_vprompt_prompts", args)


def _invoke_list(
    cfg: CliConfig,
    module: str,
    cmd: str,
    args: dict[str, Any],
    *,
    empty_is_ok: bool,
) -> CommandResult:
    try:
        data = InspirexClient(cfg).invoke(cmd, args)
    except CliClientError as e:
        env = CliEnvelope(module=module, data_source="live", warnings=[str(e)])
        return CommandResult(env, e.exit_code)
    items = _as_list(data)
    env = CliEnvelope(module=module, data_source="live")
    env.with_items(items)
    if not items and not empty_is_ok:
        return CommandResult(env, EXIT_NO_DATA)
    return CommandResult(env)


def _invoke_one(
    cfg: CliConfig,
    module: str,
    cmd: str,
    args: dict[str, Any],
    *,
    scrape: bool = False,
) -> CommandResult:
    try:
        data = InspirexClient(cfg).invoke(cmd, args)
    except CliClientError as e:
        env = CliEnvelope(module=module, data_source="live", warnings=[str(e)])
        code = e.exit_code if not scrape else EXIT_SCRAPE_FAIL
        return CommandResult(env, code)
    payload = data if isinstance(data, dict) else {"value": data}
    env = CliEnvelope(module=module, data_source="live", payload=payload)
    env.with_items([payload])
    return CommandResult(env)
