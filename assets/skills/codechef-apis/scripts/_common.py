"""Shared helpers for CodeChef API scripts."""

import json
import os
import signal
import sys

try:
    signal.signal(signal.SIGPIPE, signal.SIG_DFL)
except (AttributeError, ValueError):
    pass

BASE_URL = "https://www.codechef.com"

_BROWSER_HEADERS = {
    "accept": "application/json, text/plain, */*",
    "accept-language": "en-US,en;q=0.9",
    "sec-fetch-dest": "empty",
    "sec-fetch-mode": "cors",
    "sec-fetch-site": "same-origin",
    "user-agent": (
        "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 "
        "(KHTML, like Gecko) Chrome/152.0.0.0 Safari/537.36 Edg/152.0.0.0"
    ),
    "x-requested-with": "XMLHttpRequest",
}


def api_get(path_or_url, referer=None, params=None):
    """GET a CodeChef API endpoint and return the parsed JSON response."""
    url = path_or_url if path_or_url.startswith("http") else BASE_URL + path_or_url
    headers = dict(_BROWSER_HEADERS)
    if referer:
        headers["referer"] = referer
    try:
        text = _http_get(url, headers, params)
    except _HttpError as exc:
        fail(str(exc))
    try:
        return json.loads(text)
    except json.JSONDecodeError:
        snippet = text[:200].replace("\n", " ")
        fail(f"response is not valid JSON (maybe blocked/Cloudflare): {snippet}")


class _HttpError(Exception):
    pass


def _http_get(url, headers, params):
    try:
        import requests
    except ImportError:
        return _http_get_urllib(url, headers, params)
    try:
        resp = requests.get(url, headers=headers, params=params, timeout=30)
    except requests.RequestException as exc:
        raise _HttpError(f"request failed: {exc}") from exc
    if resp.status_code != 200:
        raise _HttpError(f"HTTP {resp.status_code} for {resp.url}")
    return resp.text


def _http_get_urllib(url, headers, params):
    import urllib.error
    import urllib.parse
    import urllib.request

    if params:
        url = url + ("&" if "?" in url else "?") + urllib.parse.urlencode(params)
    req = urllib.request.Request(url, headers=headers)
    try:
        with urllib.request.urlopen(req, timeout=30) as resp:
            return resp.read().decode("utf-8", errors="replace")
    except urllib.error.HTTPError as exc:
        raise _HttpError(f"HTTP {exc.code} for {exc.url}") from exc
    except urllib.error.URLError as exc:
        raise _HttpError(f"request failed: {exc.reason}") from exc


def fail(message, code=1):
    print(f"error: {message}", file=sys.stderr)
    sys.exit(code)


def _load_toon():
    try:
        import toon

        return toon
    except ImportError:
        pass
    vendor_dir = os.path.join(os.path.dirname(os.path.abspath(__file__)), "_vendor")
    if vendor_dir not in sys.path:
        sys.path.insert(0, vendor_dir)
    try:
        import toon

        return toon
    except ImportError:
        fail("TOON output requires python-toon; run: pip install python-toon")


def emit(data, fmt="JSON"):
    """Print structured data as compact JSON or TOON."""
    if fmt == "TOON":
        toon = _load_toon()
        print(toon.encode(data))
    else:
        print(json.dumps(data, separators=(",", ":"), ensure_ascii=False))


def add_format_arg(parser):
    parser.add_argument(
        "--format",
        choices=["JSON", "TOON"],
        default="JSON",
        help="output format: compact JSON (default) or TOON (token-efficient)",
    )
