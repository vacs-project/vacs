#!/usr/bin/env python3
"""Generate a unified THIRD_PARTY_LICENSES.html for vacs-client.

Merges:
  1. Rust crate licenses (via cargo-about, filtered to the actual dependency tree
     of a given workspace member)
  2. Node package licenses (via npx license-checker)

Usage:
    python3 scripts/generate-licenses.py                         # default: vacs-client
    python3 scripts/generate-licenses.py --package vacs-server   # server only
    python3 scripts/generate-licenses.py --output path/to/out.html
"""

from __future__ import annotations

import argparse
import html
import json
import os
import re
import string
import subprocess
import sys
from pathlib import Path
from urllib.parse import quote as urlquote

SCRIPT_DIR = Path(__file__).resolve().parent
WORKSPACE_ROOT = SCRIPT_DIR.parent
TEMPLATE_FILE = SCRIPT_DIR / "templates" / "licenses.html"


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

def run(cmd: list[str], *, cwd: Path | None = None, check: bool = True) -> str:
    result = subprocess.run(
        cmd,
        cwd=cwd or WORKSPACE_ROOT,
        capture_output=True,
        text=True,
    )
    if check and result.returncode != 0:
        print(f"ERROR: {' '.join(cmd)}", file=sys.stderr)
        print(result.stderr, file=sys.stderr)
        sys.exit(1)
    return result.stdout


def cargo_tree_deps(package: str) -> set[str]:
    """Return the set of crate names in *package*'s resolved dependency tree."""
    out = run(
        ["cargo", "tree", "-p", package, "--prefix", "none", "--edges", "normal"],
    )
    names: set[str] = set()
    for line in out.splitlines():
        line = line.strip()
        if not line or line.startswith("["):
            continue
        # Lines look like:  crate_name v1.2.3
        #              or:  crate_name v1.2.3 (*)
        #              or:  crate_name v1.2.3 (proc-macro)
        name = line.split()[0]
        names.add(name)
    return names


# ---------------------------------------------------------------------------
# Rust licenses via cargo-about
# ---------------------------------------------------------------------------

def rust_licenses(package: str) -> list[dict]:
    """Generate license data for Rust crates, filtered to *package*'s deps."""
    # 1. Get the real dependency set from cargo tree.
    allowed_crates = cargo_tree_deps(package)
    print(f"  cargo tree resolved {len(allowed_crates)} crates for {package}")

    # 2. Run cargo-about for the full workspace (it can't scope itself).
    raw = run(
        [
            "cargo", "about", "generate",
            "--workspace",
            "--format", "json",
        ],
    )
    data = json.loads(raw)

    # 3. Filter each license group to only crates in the dep tree.
    filtered: list[dict] = []
    for lic in data["licenses"]:
        used = [
            u for u in lic["used_by"]
            if u["crate"]["name"] in allowed_crates
        ]
        if used:
            entry = dict(lic)
            entry["used_by"] = used
            filtered.append(entry)

    total = sum(len(l["used_by"]) for l in filtered)
    print(f"  cargo-about: {total} crates after filtering")
    return filtered


# ---------------------------------------------------------------------------
# Node licenses via license-checker
# ---------------------------------------------------------------------------

def node_licenses(npm_dir: Path) -> list[dict]:
    """Collect npm license data and return in cargo-about-compatible format.

    Groups packages by (license, license_text) so the output structure matches
    the Rust license entries.
    """
    if not (npm_dir / "node_modules").is_dir():
        print(f"  SKIP npm: {npm_dir / 'node_modules'} not found", file=sys.stderr)
        return []

    raw = run(
        ["npx", "license-checker", "--json", "--excludePrivatePackages"],
        cwd=npm_dir,
    )
    data: dict = json.loads(raw)
    if not data:
        print("  npm: 0 packages")
        return []

    # Group by (license_id, license_text_path) → list of packages.
    groups: dict[str, dict] = {}  # key = license_id : text_hash
    for pkg_id, info in data.items():
        lic_id = info.get("licenses", "UNKNOWN")
        lic_file = info.get("licenseFile", "")
        lic_text = ""
        if lic_file and os.path.isfile(lic_file):
            try:
                lic_text = Path(lic_file).read_text(errors="replace")
            except OSError:
                pass

        # Derive name and version from "pkgname@version" format.
        match = re.match(r"^(.+)@([^@]+)$", pkg_id)
        if match:
            name, version = match.group(1), match.group(2)
        else:
            name, version = pkg_id, ""

        key = f"{lic_id}:{hash(lic_text)}"
        if key not in groups:
            groups[key] = {
                "name": _spdx_display_name(lic_id),
                "id": _normalize_spdx_id(lic_id),
                "text": lic_text,
                "used_by": [],
                "source": "npm",
            }
        groups[key]["used_by"].append({
            "crate": {  # reuse the same structure for template compat
                "name": name,
                "version": version,
                "repository": info.get("repository", ""),
            },
            "source": "npm",
        })

    result = list(groups.values())
    total = sum(len(g["used_by"]) for g in result)
    print(f"  npm: {total} packages in {len(result)} license groups")
    return result


# SPDX ID → human-readable name (matching cargo-about's naming).
_SPDX_NAMES: dict[str, str] = {
    "0BSD": "Zero-Clause BSD License",
    "Apache-2.0": "Apache License 2.0",
    "Apache-2.0 WITH LLVM-exception": "Apache License 2.0 with LLVM Exception",
    "BSD-2-Clause": 'BSD 2-Clause "Simplified" License',
    "BSD-3-Clause": 'BSD 3-Clause "New" or "Revised" License',
    "CC-BY-4.0": "Creative Commons Attribution 4.0 International",
    "CC0-1.0": "Creative Commons Zero v1.0 Universal",
    "CDLA-Permissive-2.0": "Community Data License Agreement Permissive 2.0",
    "ISC": "ISC License",
    "MIT": "MIT License",
    "MPL-2.0": "Mozilla Public License 2.0",
    "OpenSSL": "OpenSSL License",
    "Python-2.0": "Python License 2.0",
    "Unicode-3.0": "Unicode License v3",
    "Unlicense": "The Unlicense",
    "Zlib": "zlib License",
}


def _normalize_spdx_id(raw: str) -> str:
    """Normalize variations like 'Apache-2.0 OR MIT' into a stable SPDX key."""
    # license-checker sometimes returns compound expressions - keep as-is
    return raw.strip()


def _spdx_display_name(spdx_id: str) -> str:
    return _SPDX_NAMES.get(spdx_id, spdx_id)


# ---------------------------------------------------------------------------
# HTML rendering
# ---------------------------------------------------------------------------

def _aggregate_overview(entries: list[dict]) -> list[dict]:
    """Aggregate license entries by SPDX ID for the overview.

    Returns a list of dicts sorted by crate count (descending), each with:
      id, name, count, indices (positions in the flat entries list)
    """
    groups: dict[str, dict] = {}  # keyed by SPDX id
    for idx, e in enumerate(entries):
        lid = e["id"]
        if lid not in groups:
            groups[lid] = {"id": lid, "name": e["name"], "count": 0, "indices": []}
        groups[lid]["count"] += len(e["used_by"])
        groups[lid]["indices"].append(idx)
    return sorted(groups.values(), key=lambda g: -g["count"])


def render_html(
    rust: list[dict],
    node: list[dict],
    title: str = "Third-Party Licenses",
) -> str:
    """Render a standalone HTML page from the license data.

    Mirrors cargo-about's output style:
    - Overview: one row per SPDX license ID with total crate count.
    - Detail: one block per text variant, each with its own "Used by"
      list and license text - so it's clear which dependency uses which
      variant.
    """

    all_entries = rust + node
    overview = _aggregate_overview(all_entries)

    rust_count = sum(len(e["used_by"]) for e in rust)
    node_count = sum(len(e["used_by"]) for e in node)
    subtitle = f"{rust_count} Rust crates"
    if node_count:
        subtitle += f" and {node_count} Node packages"

    # --- Overview list ---
    overview_items: list[str] = []
    for g in overview:
        frag = urlquote(g["id"], safe="")
        name = html.escape(g["name"])
        overview_items.append(
            f'            <li><a href="#{frag}">{name}</a> ({g["count"]})</li>'
        )

    # --- Detailed license blocks ---
    # Each entry (text variant) gets its own block, ordered by overview group.
    license_blocks: list[str] = []
    for g in overview:
        first = True
        frag = urlquote(g["id"], safe="")
        name = html.escape(g["name"])
        for idx in g["indices"]:
            entry = all_entries[idx]
            used_by = sorted(entry["used_by"], key=lambda u: u["crate"]["name"].lower())

            used_by_items = "\n".join(
                f'                    <li><a href="{html.escape(_crate_url(u))}">' 
                f'{html.escape(u["crate"]["name"])} {html.escape(u["crate"]["version"])}</a></li>'
                for u in used_by
            )

            text = entry.get("text", "").strip()
            text_html = (
                f'<pre class="license-text">{html.escape(text)}</pre>'
                if text
                else '<pre class="license-text">(license text not available)</pre>'
            )

            # The first block for each SPDX ID gets the anchor for the overview link.
            # Use the raw ID (HTML-escaped) for the id attribute - browsers
            # URL-decode href fragments before matching against id values.
            anchor = f' id="{html.escape(g["id"])}"' if first else ""
            first = False

            license_blocks.append(
                f"""            <li class="license">
                <h3{anchor}>{name}</h3>
                <h4>Used by:</h4>
                <ul class="license-used-by">
{used_by_items}
                </ul>
                {text_html}
            </li>"""
            )

    template = string.Template(TEMPLATE_FILE.read_text(encoding="utf-8"))
    return template.substitute(
        title=html.escape(title),
        subtitle=html.escape(subtitle),
        overview_items="\n".join(overview_items),
        license_blocks="\n".join(license_blocks),
    )


def _crate_url(u: dict) -> str:
    repo = u["crate"].get("repository", "")
    if repo:
        return repo
    if u.get("source") == "npm":
        return f"https://www.npmjs.com/package/{u['crate']['name']}"
    return f"https://crates.io/crates/{u['crate']['name']}"


# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------

def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--package", "-p",
        default="vacs-client",
        help="Cargo workspace member to scope deps to (default: vacs-client)",
    )
    parser.add_argument(
        "--output", "-o",
        default=str(WORKSPACE_ROOT / "THIRD_PARTY_LICENSES.html"),
        help="Output file path (default: THIRD_PARTY_LICENSES.html in workspace root)",
    )
    args = parser.parse_args()

    # Auto-detect npm directory: if the package has node_modules, include npm licenses.
    pkg_dir = WORKSPACE_ROOT / args.package
    npm_dir = pkg_dir if (pkg_dir / "node_modules").is_dir() else None

    print(f"Generating third-party licenses for {args.package}...")
    print()

    print("[1/2] Collecting Rust crate licenses...")
    rust = rust_licenses(args.package)

    node: list[dict] = []
    if npm_dir is not None:
        print("[2/2] Collecting Node package licenses...")
        node = node_licenses(npm_dir)
    else:
        print("[2/2] Skipping Node package licenses (no npm directory)")

    print()
    print("Rendering HTML...")
    html = render_html(rust, node)

    out = Path(args.output)
    out.write_text(html, encoding="utf-8")
    print(f"Written to {out} ({len(html):,} bytes)")


if __name__ == "__main__":
    main()
