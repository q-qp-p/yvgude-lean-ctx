import sys
import threading
from http.server import BaseHTTPRequestHandler, HTTPServer
from pathlib import Path

import pytest

ROOT = Path(__file__).parent
SDK_ROOT = ROOT.parent.parent / "packages" / "python-lean-ctx"
if str(SDK_ROOT) not in sys.path:
    sys.path.insert(0, str(SDK_ROOT))

from poc import _proxy_reachable, cmd_doctor  # noqa: E402


class _HealthHandler(BaseHTTPRequestHandler):
    def do_GET(self):  # noqa: N802
        self.server.paths.append(self.path)
        self.server.headers.append(dict(self.headers))
        self.send_response(200)
        self.end_headers()

    def log_message(self, *args):
        pass


@pytest.fixture
def health_server():
    httpd = HTTPServer(("127.0.0.1", 0), _HealthHandler)
    httpd.paths = []
    httpd.headers = []
    thread = threading.Thread(target=httpd.serve_forever, daemon=True)
    thread.start()
    try:
        host, port = httpd.server_address
        yield httpd, f"http://{host}:{port}"
    finally:
        httpd.shutdown()
        thread.join(timeout=2)


def test_doctor_probes_resolved_sdk_endpoint(monkeypatch, health_server):
    httpd, base_url = health_server
    monkeypatch.setenv("LEAN_CTX_PROXY_URL", base_url)
    monkeypatch.setenv("LEAN_CTX_PROXY_TOKEN", "doctor-secret")

    assert _proxy_reachable() is True
    assert httpd.paths == ["/health"]
    assert "Authorization" not in httpd.headers[0]


def test_doctor_warns_when_resolved_sdk_endpoint_is_unreachable(monkeypatch, capsys):
    monkeypatch.setenv("LEAN_CTX_PROXY_URL", "http://127.0.0.1:1")

    assert cmd_doctor() == 0
    assert "WARN  lean-ctx proxy: not reachable" in capsys.readouterr().out


def test_doctor_reports_reachable_resolved_sdk_endpoint(monkeypatch, health_server, capsys):
    httpd, base_url = health_server
    monkeypatch.setenv("LEAN_CTX_PROXY_URL", base_url)

    assert cmd_doctor() == 0
    output = capsys.readouterr().out
    assert f"ok    lean-ctx proxy: loopback ({base_url})" in output
    assert httpd.paths == ["/health"]


def test_doctor_reports_malformed_endpoint_without_hiding_import_errors(monkeypatch, capsys):
    monkeypatch.setenv("LEAN_CTX_PROXY_URL", "http://[::1")

    assert cmd_doctor() == 0
    output = capsys.readouterr().out
    assert "WARN  lean-ctx proxy: not reachable (http://[::1)" in output


def test_proxy_reachable_does_not_swallow_unexpected_errors(monkeypatch):
    import lean_ctx

    class BrokenProxyClient:
        def __init__(self, **kwargs):
            raise RuntimeError("unexpected construction failure")

    monkeypatch.setattr(lean_ctx, "ProxyClient", BrokenProxyClient)
    with pytest.raises(RuntimeError, match="unexpected construction failure"):
        _proxy_reachable()
