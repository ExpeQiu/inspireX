"""JSON 输出契约（对齐 15CLI/CLI标准.md §10）。"""

from __future__ import annotations

from datetime import datetime, timezone, timedelta
from typing import Any, Literal, Optional

from pydantic import BaseModel, Field

from inspirex_cli.__version__ import __version__

TZ_SH = timezone(timedelta(hours=8))


def now_iso() -> str:
    return datetime.now(TZ_SH).replace(microsecond=0).isoformat()


class CliEnvelope(BaseModel):
    module: str
    version: str = __version__
    data_source: Literal["live", "demo", "cache"]
    fetched_at: str = Field(default_factory=now_iso)
    count: int = 0
    items: list[Any] = Field(default_factory=list)
    warnings: list[str] = Field(default_factory=list)
    payload: Optional[dict[str, Any]] = None

    def with_items(self, items: list[Any]) -> "CliEnvelope":
        self.items = items
        self.count = len(items)
        return self
