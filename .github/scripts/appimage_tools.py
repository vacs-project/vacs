#!/usr/bin/env python3
"""Pins the tools tauri-bundler downloads for the AppImage bundle.

tauri-bundler fetches them from rolling upstream tags with no checksum and runs them in the
release build next to the signing key, but skips any download whose file is already in its
cache. Seeding the cache with verified copies keeps a changed upstream file from shipping
under our signature.

    fetch DIR       download every pinned tool into DIR and verify it
    verify          download every pinned tool and report hash mismatches
    check-bundler   compare the pins with what the locked @tauri-apps/cli would download
"""

import hashlib
import json
import re
import sys
import tempfile
import urllib.request
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
MANIFEST = REPO / ".github" / "appimage-tools.md"
PACKAGE_LOCK = REPO / "vacs-client" / "package-lock.json"
BUNDLER_SOURCE = (
    "https://raw.githubusercontent.com/tauri-apps/tauri/tauri-cli-v{version}"
    "/crates/tauri-bundler/src/bundle/linux/appimage/linuxdeploy.rs"
)
ARCH = "x86_64"


def read_manifest():
    entries = []
    for line in MANIFEST.read_text().splitlines():
        if not line.startswith("|"):
            continue
        cells = [cell.strip().strip("`") for cell in line.strip("|").split("|")]
        if cells[0] in ("Cache file", "---"):
            continue
        name, sha256, url, bundler_url = cells
        entries.append({"name": name, "sha256": sha256, "url": url, "bundler_url": bundler_url or url})
    return entries


def download(url):
    request = urllib.request.Request(url, headers={"User-Agent": "vacs-appimage-tools"})
    for attempt in range(3):
        try:
            with urllib.request.urlopen(request, timeout=60) as response:
                return response.read()
        except OSError:
            if attempt == 2:
                raise
    raise AssertionError("unreachable")


def fetch_into(directory):
    directory.mkdir(parents=True, exist_ok=True)
    mismatches = []
    for entry in read_manifest():
        data = download(entry["url"])
        actual = hashlib.sha256(data).hexdigest()
        if actual != entry["sha256"]:
            mismatches.append(f"{entry['name']}: expected {entry['sha256']}, got {actual} from {entry['url']}")
            continue
        path = directory / entry["name"]
        path.write_bytes(data)
        path.chmod(0o755)
    return mismatches


def bundler_downloads(source):
    """Returns (cache file name, URL) for every tool `prepare_tools` downloads."""
    body = re.search(r"fn prepare_tools\(.*?\n}\n", source, re.S)
    if body is None:
        raise SystemExit("prepare_tools not found in the bundler source; re-check the pins by hand")
    body = body.group(0)

    substitutions = {"arch": ARCH, "linuxdeploy_arch": ARCH, "tools_arch": ARCH}
    for const, value in re.findall(r'const (\w+): &str = "([^"]+)";', body):
        substitutions[const] = value

    def expand(template):
        return re.sub(r"\{(\w+)\}", lambda m: substitutions[m.group(1)], template)

    names = [
        expand(m.group(1) or m.group(2))
        for m in re.finditer(r'tools_path\.join\(\s*(?:format!\(\s*"([^"]+)"|"([^"]+)")', body)
    ]
    urls = [expand(url) for url in re.findall(r'"(https://[^"]+)"', body)]
    if not names or len(names) != len(urls):
        raise SystemExit(
            f"could not pair the bundler's downloads (names {names}, URLs {urls}); "
            "re-check the pins by hand"
        )
    return sorted(zip(names, urls))


def check_bundler():
    lock = json.loads(PACKAGE_LOCK.read_text())
    version = lock["packages"]["node_modules/@tauri-apps/cli"]["version"]
    source = download(BUNDLER_SOURCE.format(version=version)).decode()

    expected = bundler_downloads(source)
    pinned = sorted((entry["name"], entry["bundler_url"]) for entry in read_manifest())
    if expected == pinned:
        print(f"Pins match the AppImage tools of @tauri-apps/cli {version}")
        return 0

    print(f"Pins do not match the AppImage tools of @tauri-apps/cli {version}:", file=sys.stderr)
    for name, url in sorted(set(expected) - set(pinned)):
        print(f"  unpinned: {name} from {url}", file=sys.stderr)
    for name, url in sorted(set(pinned) - set(expected)):
        print(f"  no longer downloaded: {name} from {url}", file=sys.stderr)
    print(f"Update {MANIFEST.relative_to(REPO)} after reviewing the new upstream files.", file=sys.stderr)
    return 1


def main(argv):
    match argv:
        case ["fetch", directory]:
            mismatches = fetch_into(Path(directory))
        case ["verify"]:
            with tempfile.TemporaryDirectory() as directory:
                mismatches = fetch_into(Path(directory))
        case ["check-bundler"]:
            return check_bundler()
        case _:
            print(__doc__, file=sys.stderr)
            return 2

    for mismatch in mismatches:
        print(f"::error::{mismatch}", file=sys.stderr)
    return 1 if mismatches else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
