"""配置：CLI 参数 > 环境变量 > ~/.inspirex/config.yaml > 项目 .env > 默认值。"""

from __future__ import annotations

import os
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Mapping, Optional

import yaml

DEFAULT_PORT = 18765
DEFAULT_TOKEN = "inspirex-local"
USER_CONFIG_DIR = Path.home() / ".inspirex"
USER_CONFIG_PATH = USER_CONFIG_DIR / "config.yaml"


def _truthy(value: Optional[str]) -> bool:
    if value is None:
        return False
    return value.strip().lower() in {"1", "true", "yes", "on"}


def _load_dotenv(path: Path) -> dict[str, str]:
    out: dict[str, str] = {}
    if not path.is_file():
        return out
    for raw in path.read_text(encoding="utf-8").splitlines():
        line = raw.strip()
        if not line or line.startswith("#") or "=" not in line:
            continue
        key, _, val = line.partition("=")
        key = key.strip()
        val = val.strip().strip("'").strip('"')
        if key and key not in os.environ:
            out[key] = val
    return out


def _load_yaml(path: Path) -> dict[str, Any]:
    if not path.is_file():
        return {}
    data = yaml.safe_load(path.read_text(encoding="utf-8")) or {}
    return data if isinstance(data, dict) else {}


def find_project_root(start: Optional[Path] = None) -> Optional[Path]:
    cur = (start or Path.cwd()).resolve()
    for candidate in [cur, *cur.parents]:
        if (candidate / "src-tauri" / "Cargo.toml").is_file() and (
            candidate / ".env.example"
        ).is_file():
            return candidate
    return None


def init_user_config() -> Path:
    USER_CONFIG_DIR.mkdir(parents=True, exist_ok=True)
    if not USER_CONFIG_PATH.exists():
        USER_CONFIG_PATH.write_text(
            "ingest_port: 18765\n"
            "ingest_token: inspirex-local\n"
            "mock_mode: true\n",
            encoding="utf-8",
        )
    return USER_CONFIG_PATH


@dataclass
class CliConfig:
    ingest_port: int = DEFAULT_PORT
    ingest_token: str = DEFAULT_TOKEN
    library_path: str = ""
    mock_mode: bool = True
    base_url: str = ""
    project_root: Optional[Path] = None

    @property
    def origin(self) -> str:
        if self.base_url:
            return self.base_url.rstrip("/")
        return f"http://127.0.0.1:{self.ingest_port}"


def load_config(
    *,
    port: Optional[int] = None,
    token: Optional[str] = None,
    base_url: Optional[str] = None,
    demo: bool = False,
    env: Optional[Mapping[str, str]] = None,
) -> CliConfig:
    environ = dict(env) if env is not None else dict(os.environ)
    project = find_project_root()
    file_env: dict[str, str] = {}
    if project:
        file_env = _load_dotenv(project / ".env")
        for k, v in file_env.items():
            environ.setdefault(k, v)
    user_yaml = _load_yaml(USER_CONFIG_PATH)

    ingest_port = (
        port
        or _int_or_none(environ.get("INSPIREX_INGEST_PORT"))
        or _int_or_none(str(user_yaml.get("ingest_port") or ""))
        or DEFAULT_PORT
    )
    ingest_token = (
        token
        or environ.get("INSPIREX_INGEST_TOKEN")
        or str(user_yaml.get("ingest_token") or "")
        or DEFAULT_TOKEN
    )
    library_path = (
        environ.get("INSPIREX_LIBRARY_PATH")
        or str(user_yaml.get("library_path") or "")
        or ""
    )
    if library_path == "~":
        library_path = ""
    mock_env = environ.get("INSPIREX_MOCK_MODE") or environ.get("AI_MOCK_MODE")
    mock_mode = demo or _truthy(mock_env) or bool(user_yaml.get("mock_mode", True))
    resolved_base = (
        (base_url or "").strip()
        or (environ.get("INSPIREX_CLI_BASE") or "").strip()
        or str(user_yaml.get("base_url") or "").strip()
    )
    return CliConfig(
        ingest_port=ingest_port,
        ingest_token=ingest_token,
        library_path=library_path,
        mock_mode=mock_mode,
        base_url=resolved_base,
        project_root=project,
    )


def _int_or_none(raw: Optional[str]) -> Optional[int]:
    if not raw:
        return None
    try:
        return int(raw)
    except ValueError:
        return None


def config_summary(cfg: CliConfig, *, backend_ok: Optional[bool] = None) -> dict[str, Any]:
    token_set = bool(cfg.ingest_token)
    return {
        "origin": cfg.origin,
        "ingestPort": cfg.ingest_port,
        "tokenSet": token_set,
        "libraryPath": cfg.library_path or "(默认 ~/InspireX/Library)",
        "mockMode": cfg.mock_mode,
        "userConfig": str(USER_CONFIG_PATH),
        "projectRoot": str(cfg.project_root) if cfg.project_root else "",
        "backend": "up" if backend_ok else ("down" if backend_ok is False else "unknown"),
    }
