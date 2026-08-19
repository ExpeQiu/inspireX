"""inspirex CLI 入口。日志 stderr，数据 stdout。"""

from __future__ import annotations

import sys
from typing import Optional

import click

from inspirex_cli import ops
from inspirex_cli.__version__ import __version__
from inspirex_cli.config import load_config, init_user_config
from inspirex_cli.output import FORMATS, emit
from inspirex_cli.utils.errors import EXIT_ERROR, EXIT_OK
from inspirex_cli.utils.logger import setup_logger

logger = setup_logger()


def _finish(ctx: click.Context, result: ops.CommandResult) -> None:
    emit(result.envelope, ctx.obj["fmt"], ctx.obj["output"])
    if result.exit_code != EXIT_OK:
        ctx.exit(result.exit_code)


@click.group()
@click.version_option(version=__version__, prog_name="inspirex")
@click.option("-v", "--verbose", is_flag=True, help="DEBUG 日志")
@click.option("-q", "--quiet", is_flag=True, help="仅 WARNING 及以上")
@click.option(
    "--format",
    "fmt",
    type=click.Choice(FORMATS),
    default="table",
    show_default=True,
    help="输出格式",
)
@click.option("-o", "--output", default=None, help="写入文件；默认 stdout")
@click.option("--demo", is_flag=True, help="离线夹具，不访问本机后端")
@click.option("--port", type=int, default=None, help="ingest 端口，默认 18765")
@click.option("--base", default=None, help="覆盖后端 URL，如 http://127.0.0.1:18765")
@click.pass_context
def cli(
    ctx: click.Context,
    verbose: bool,
    quiet: bool,
    fmt: str,
    output: Optional[str],
    demo: bool,
    port: Optional[int],
    base: Optional[str],
) -> None:
    """灵犀 InspireX 命令行：查询素材库、检索、入库。"""
    ctx.ensure_object(dict)
    setup_logger(verbose=verbose, quiet=quiet)
    cfg = load_config(port=port, base_url=base, demo=demo)
    ctx.obj["cfg"] = cfg
    ctx.obj["demo"] = demo
    ctx.obj["fmt"] = fmt
    ctx.obj["output"] = output
    logger.debug("cli_boot demo=%s origin=%s format=%s", demo, cfg.origin, fmt)


@cli.group()
def config() -> None:
    """配置"""


@config.command("init")
def config_init() -> None:
    """写入 ~/.inspirex/config.yaml"""
    path = init_user_config()
    click.echo(f"配置已初始化: {path}", err=True)


@config.command("check")
@click.pass_context
def config_check(ctx: click.Context) -> None:
    """检查配置与后端可达性（后端未启动不失败）"""
    _finish(ctx, ops.config_check(ctx.obj["cfg"], demo_mode=ctx.obj["demo"]))


@cli.command("status")
@click.pass_context
def status_cmd(ctx: click.Context) -> None:
    """健康检查 + 库信息"""
    _finish(ctx, ops.status(ctx.obj["cfg"], demo_mode=ctx.obj["demo"]))


@cli.group()
def library() -> None:
    """素材库"""


@library.command("info")
@click.pass_context
def library_info_cmd(ctx: click.Context) -> None:
    """库路径、素材数、CLIP / 生图模式"""
    _finish(ctx, ops.library_info(ctx.obj["cfg"], demo_mode=ctx.obj["demo"]))


@cli.group()
def asset() -> None:
    """素材"""


@asset.command("list")
@click.option("--folder-id", default=None, help="按文件夹过滤")
@click.option("--limit", default=50, show_default=True, type=int)
@click.pass_context
def asset_list_cmd(ctx: click.Context, folder_id: Optional[str], limit: int) -> None:
    """列出素材"""
    _finish(
        ctx,
        ops.asset_list(ctx.obj["cfg"], demo_mode=ctx.obj["demo"], folder_id=folder_id, limit=limit),
    )


@asset.command("search")
@click.argument("text", required=False, default="")
@click.option("--tag", default=None, help="按标签")
@click.option("--color", default=None, help="主色 #RRGGBB")
@click.option("--clip", is_flag=True, help="以文搜图（CLIP）")
@click.option("--limit", default=20, show_default=True, type=int)
@click.pass_context
def asset_search_cmd(
    ctx: click.Context,
    text: str,
    tag: Optional[str],
    color: Optional[str],
    clip: bool,
    limit: int,
) -> None:
    """检索素材（FTS / 标签 / 颜色 / CLIP）"""
    _finish(
        ctx,
        ops.asset_search(
            ctx.obj["cfg"],
            demo_mode=ctx.obj["demo"],
            text=text,
            tag=tag,
            color=color,
            clip=clip,
            limit=limit,
        ),
    )


@asset.command("import")
@click.argument("paths", nargs=-1, required=True)
@click.option("--folder-id", default=None)
@click.option("--dry-run", is_flag=True, help="只预览不入库")
@click.pass_context
def asset_import_cmd(
    ctx: click.Context,
    paths: tuple[str, ...],
    folder_id: Optional[str],
    dry_run: bool,
) -> None:
    """从本机路径入库"""
    _finish(
        ctx,
        ops.asset_import(
            ctx.obj["cfg"],
            demo_mode=ctx.obj["demo"],
            paths=list(paths),
            folder_id=folder_id,
            dry_run=dry_run,
        ),
    )


@asset.command("import-url")
@click.argument("image_url")
@click.option("--page-url", default=None)
@click.option("--title", default=None)
@click.option("--folder-id", default=None)
@click.pass_context
def asset_import_url_cmd(
    ctx: click.Context,
    image_url: str,
    page_url: Optional[str],
    title: Optional[str],
    folder_id: Optional[str],
) -> None:
    """从图片 URL 入库"""
    _finish(
        ctx,
        ops.asset_import_url(
            ctx.obj["cfg"],
            demo_mode=ctx.obj["demo"],
            image_url=image_url,
            page_url=page_url,
            title=title,
            folder_id=folder_id,
        ),
    )


@cli.group()
def folder() -> None:
    """文件夹"""


@folder.command("list")
@click.pass_context
def folder_list_cmd(ctx: click.Context) -> None:
    """列出文件夹"""
    _finish(ctx, ops.folder_list(ctx.obj["cfg"], demo_mode=ctx.obj["demo"]))


@folder.command("create")
@click.argument("name")
@click.option("--parent-id", default=None)
@click.pass_context
def folder_create_cmd(ctx: click.Context, name: str, parent_id: Optional[str]) -> None:
    """新建文件夹"""
    _finish(
        ctx,
        ops.folder_create(ctx.obj["cfg"], demo_mode=ctx.obj["demo"], name=name, parent_id=parent_id),
    )


@cli.group()
def tag() -> None:
    """标签"""


@tag.command("list")
@click.pass_context
def tag_list_cmd(ctx: click.Context) -> None:
    """列出标签"""
    _finish(ctx, ops.tag_list(ctx.obj["cfg"], demo_mode=ctx.obj["demo"]))


@tag.command("create")
@click.argument("name")
@click.option("--color", default=None, help="#RRGGBB")
@click.pass_context
def tag_create_cmd(ctx: click.Context, name: str, color: Optional[str]) -> None:
    """新建标签"""
    _finish(ctx, ops.tag_create(ctx.obj["cfg"], demo_mode=ctx.obj["demo"], name=name, color=color))


@cli.group()
def source() -> None:
    """订阅源"""


@source.command("list")
@click.pass_context
def source_list_cmd(ctx: click.Context) -> None:
    """列出订阅源"""
    _finish(ctx, ops.source_list(ctx.obj["cfg"], demo_mode=ctx.obj["demo"]))


@source.command("pull")
@click.argument("source_id")
@click.pass_context
def source_pull_cmd(ctx: click.Context, source_id: str) -> None:
    """立即拉取订阅源"""
    _finish(ctx, ops.source_pull(ctx.obj["cfg"], demo_mode=ctx.obj["demo"], source_id=source_id))


@cli.group()
def hub() -> None:
    """中转站"""


@hub.command("status")
@click.pass_context
def hub_status_cmd(ctx: click.Context) -> None:
    """中转站绑定状态"""
    _finish(ctx, ops.hub_status(ctx.obj["cfg"], demo_mode=ctx.obj["demo"]))


def main() -> int:
    try:
        cli(standalone_mode=True)
        return EXIT_OK
    except SystemExit as e:
        code = e.code
        if code is None:
            return EXIT_OK
        if isinstance(code, int):
            return code
        return EXIT_ERROR


if __name__ == "__main__":
    sys.exit(main())
