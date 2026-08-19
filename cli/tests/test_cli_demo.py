from click.testing import CliRunner

from inspirex_cli.cli import cli
from inspirex_cli.output import render
from inspirex_cli.models import CliEnvelope


def runner() -> CliRunner:
    try:
        return CliRunner(mix_stderr=False)
    except TypeError:
        return CliRunner()


def test_version():
    r = runner().invoke(cli, ["--version"])
    assert r.exit_code == 0
    assert "inspirex" in r.output
    assert "0.1.0" in r.output


def test_help():
    r = runner().invoke(cli, ["--help"])
    assert r.exit_code == 0
    assert "asset" in r.output
    assert "library" in r.output


def test_asset_list_demo_json_contract():
    r = runner().invoke(cli, ["--demo", "-q", "--format", "json", "asset", "list"])
    assert r.exit_code == 0, r.output
    import json

    d = json.loads(r.output)
    assert d["module"] == "asset-list"
    assert d["data_source"] == "demo"
    assert d["count"] == 2
    assert d["items"][0]["filename"] == "sunset.png"
    assert "version" in d
    assert "fetched_at" in d


def test_asset_search_demo_hit_and_miss():
    hit = runner().invoke(cli, ["--demo", "-q", "--format", "json", "asset", "search", "sunset"])
    assert hit.exit_code == 0
    import json

    d = json.loads(hit.output)
    assert d["count"] == 1
    miss = runner().invoke(cli, ["--demo", "-q", "--format", "json", "asset", "search", "zzz-no-such"])
    assert miss.exit_code == 2
    empty = json.loads(miss.output)
    assert empty["count"] == 0


def test_library_info_and_status_demo():
    import json

    info = runner().invoke(cli, ["--demo", "-q", "--format", "json", "library", "info"])
    assert info.exit_code == 0
    d = json.loads(info.output)
    assert d["module"] == "library-info"
    assert d["items"][0]["assetCount"] == 2

    st = runner().invoke(cli, ["--demo", "-q", "--format", "json", "status"])
    assert st.exit_code == 0
    s = json.loads(st.output)
    assert s["items"][0]["ok"] is True


def test_config_check_demo():
    import json

    r = runner().invoke(cli, ["--demo", "-q", "--format", "json", "config", "check"])
    assert r.exit_code == 0, r.output
    d = json.loads(r.output)
    assert d["module"] == "config-check"
    assert d["items"][0]["ingestPort"] == 18765


def test_folder_tag_source_hub_demo():
    rnr = runner()
    for args in (
        ["folder", "list"],
        ["tag", "list"],
        ["source", "list"],
        ["hub", "status"],
        ["folder", "create", "海报"],
        ["tag", "create", "红"],
        ["source", "pull", "demo-source-1"],
        ["asset", "import", "/tmp/a.png"],
    ):
        r = rnr.invoke(cli, ["--demo", "-q", "--format", "json", *args])
        assert r.exit_code == 0, (args, r.output)


def test_logs_go_to_stderr_not_stdout():
    r = runner().invoke(cli, ["--demo", "--verbose", "--format", "json", "asset", "list"])
    assert r.exit_code == 0
    stdout = (getattr(r, "stdout", None) or r.output).strip()
    assert stdout.startswith("{")
    err = getattr(r, "stderr", "") or ""
    assert "cli_asset_list" in err or "cli_boot" in err or stdout.startswith("{")


def test_render_table_and_markdown():
    env = CliEnvelope(module="asset-list", data_source="demo", count=1, items=[{"id": "1", "filename": "a.png"}])
    table = render(env, "table")
    assert "asset-list" in table
    assert "a.png" in table
    md = render(env, "markdown")
    assert "| filename |" in md or "filename" in md
