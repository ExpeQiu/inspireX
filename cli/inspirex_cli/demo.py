"""离线 --demo 夹具，verify 不依赖 ingest。"""

from __future__ import annotations

from typing import Any


def demo_library_info() -> dict[str, Any]:
    return {
        "libraryPath": "/tmp/inspirex-demo/Library",
        "aiMockMode": True,
        "assetCount": 2,
        "ingestPort": 18765,
        "clipModel": "mock",
        "imageGenProvider": "mock",
    }


def demo_assets() -> list[dict[str, Any]]:
    return [
        {
            "id": "demo-asset-1",
            "filename": "sunset.png",
            "storedPath": "/tmp/inspirex-demo/Library/ingest/sunset.png",
            "thumbPath": None,
            "mime": "image/png",
            "width": 8,
            "height": 8,
            "fileSize": 128,
            "sourceUrl": "https://example.com/sunset.png",
            "notes": "demo",
            "folderId": "demo-folder-1",
            "dominantColor": "#C85A3A",
            "clipStatus": "ready",
            "createdAt": "2026-08-19T11:00:00+08:00",
            "tags": ["风景"],
            "sourceKind": "import",
            "sourceRef": None,
        },
        {
            "id": "demo-asset-2",
            "filename": "poster.jpg",
            "storedPath": "/tmp/inspirex-demo/Library/ingest/poster.jpg",
            "thumbPath": None,
            "mime": "image/jpeg",
            "width": 16,
            "height": 16,
            "fileSize": 256,
            "sourceUrl": None,
            "notes": None,
            "folderId": None,
            "dominantColor": "#2E5BFF",
            "clipStatus": "ready",
            "createdAt": "2026-08-19T11:01:00+08:00",
            "tags": ["海报"],
            "sourceKind": "plugin",
            "sourceRef": None,
        },
    ]


def demo_folders() -> list[dict[str, Any]]:
    return [
        {
            "id": "demo-folder-1",
            "name": "风景",
            "parentId": None,
            "createdAt": "2026-08-19T10:00:00+08:00",
            "itemCount": 1,
        }
    ]


def demo_tags() -> list[dict[str, Any]]:
    return [
        {
            "id": "demo-tag-1",
            "name": "风景",
            "color": "#C85A3A",
            "createdAt": "2026-08-19T10:00:00+08:00",
        }
    ]


def demo_sources() -> list[dict[str, Any]]:
    return [
        {
            "id": "demo-source-1",
            "name": "示例 RSS",
            "kind": "rss",
            "url": "https://example.com/feed.xml",
            "enabled": True,
            "intervalMin": 360,
        }
    ]


def demo_hub_status() -> dict[str, Any]:
    return {
        "bound": True,
        "path": "/tmp/inspirex-demo/Hub",
        "kind": "icloud",
    }


def demo_import_result(paths: list[str]) -> dict[str, Any]:
    return {
        "imported": len(paths),
        "skipped": 0,
        "errors": [],
        "runId": "demo-run",
        "dryRun": True,
        "paths": list(paths),
    }


def demo_health() -> dict[str, Any]:
    return {"ok": True, "service": "inspirex-ingest", "demo": True}


def demo_vprompt_status() -> dict[str, Any]:
    return {
        "root": "/Volumes/Lexar/git/12 highvalue/VPromptStore",
        "ready": True,
        "lastSyncAt": "2026-08-24T00:00:00+08:00",
        "lastRunId": "demo-sync",
        "lastInserted": 220,
        "lastUpdated": 0,
        "lastPruned": 0,
        "lastTotal": 220,
    }


def demo_vprompt_sync() -> dict[str, Any]:
    return {
        "runId": "demo-sync",
        "root": "/Volumes/Lexar/git/12 highvalue/VPromptStore",
        "categories": 11,
        "inserted": 0,
        "updated": 220,
        "pruned": 0,
        "total": 220,
        "skipped": 0,
    }
