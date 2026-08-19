from inspirex_cli.config import load_config


def test_env_overrides_yaml(monkeypatch):
    monkeypatch.setenv("INSPIREX_INGEST_PORT", "19999")
    monkeypatch.setenv("INSPIREX_CLI_BASE", "http://127.0.0.1:19999")
    monkeypatch.setenv("INSPIREX_INGEST_TOKEN", "tok")
    cfg = load_config(env=dict(**{**__import__("os").environ}))
    assert cfg.ingest_port == 19999
    assert cfg.origin == "http://127.0.0.1:19999"
    assert cfg.ingest_token == "tok"


def test_cli_port_flag_wins(monkeypatch):
    monkeypatch.setenv("INSPIREX_INGEST_PORT", "11111")
    cfg = load_config(port=22222, env=dict(**__import__("os").environ))
    assert cfg.ingest_port == 22222
    assert cfg.origin.endswith(":22222")
