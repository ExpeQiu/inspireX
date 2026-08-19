"""本机 HTTP 客户端：/health 与 POST /v1/invoke。"""

from __future__ import annotations

from typing import Any, Optional

import httpx

from inspirex_cli.config import CliConfig
from inspirex_cli.utils.errors import EXIT_ERROR, EXIT_SCRAPE_FAIL
from inspirex_cli.utils.logger import setup_logger

logger = setup_logger()


class CliClientError(Exception):
    def __init__(self, message: str, exit_code: int = EXIT_ERROR):
        super().__init__(message)
        self.exit_code = exit_code


class InspirexClient:
    def __init__(self, cfg: CliConfig, timeout: float = 30.0):
        self.cfg = cfg
        self.timeout = timeout

    def health(self) -> dict[str, Any]:
        url = f"{self.cfg.origin}/health"
        try:
            with httpx.Client(timeout=self.timeout) as client:
                res = client.get(url)
                res.raise_for_status()
                data = res.json()
                logger.debug("health_ok origin=%s", self.cfg.origin)
                return data if isinstance(data, dict) else {"ok": True}
        except httpx.HTTPError as e:
            logger.warning("health_fail origin=%s err=%s", self.cfg.origin, e)
            raise CliClientError(
                f"后端不可达 {url}。请先 ./scripts/start.sh",
                EXIT_ERROR,
            ) from e

    def invoke(self, cmd: str, args: Optional[dict[str, Any]] = None) -> Any:
        url = f"{self.cfg.origin}/v1/invoke"
        payload = {"cmd": cmd, "args": args or {}}
        logger.info("rpc_invoke cmd=%s origin=%s", cmd, self.cfg.origin)
        try:
            with httpx.Client(timeout=self.timeout) as client:
                res = client.post(url, json=payload, headers=self._headers())
        except httpx.HTTPError as e:
            logger.warning("rpc_network_fail cmd=%s err=%s", cmd, e)
            raise CliClientError(
                f"后端不可达 {url}。请先 ./scripts/start.sh",
                EXIT_ERROR,
            ) from e
        if res.status_code >= 400:
            detail = _error_detail(res)
            logger.warning("rpc_fail cmd=%s status=%s err=%s", cmd, res.status_code, detail)
            code = EXIT_SCRAPE_FAIL if cmd.startswith("import") or cmd == "pull_source" else EXIT_ERROR
            raise CliClientError(detail, code)
        if not res.content:
            return None
        return res.json()

    def _headers(self) -> dict[str, str]:
        headers = {"Accept": "application/json", "Content-Type": "application/json"}
        if self.cfg.ingest_token:
            headers["Authorization"] = f"Bearer {self.cfg.ingest_token}"
        return headers


def _error_detail(res: httpx.Response) -> str:
    try:
        body = res.json()
        if isinstance(body, dict):
            return str(body.get("error") or body.get("detail") or res.text)
    except ValueError:
        pass
    return res.text or f"HTTP {res.status_code}"
