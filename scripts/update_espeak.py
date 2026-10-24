#!/usr/bin/env python3
"""Re-pins espeak-ng.lock, the espeak-ng source that build.rs downloads and verifies.

Usage: update_espeak.py [COMMIT_OR_REF]   (default: upstream master)
"""
import gzip
import hashlib
import json
import os
import sys
import urllib.request
from pathlib import Path

LOCK = Path(__file__).resolve().parent.parent / "espeak-ng.lock"
REPO = "espeak-ng/espeak-ng"
HEADER = "# espeak-ng source that build.rs downloads and verifies. Re-pin with scripts/update_espeak.py.\n"


def download(url: str) -> bytes:
    headers = {"User-Agent": "update_espeak.py"}
    token = os.environ.get("GITHUB_TOKEN")
    if token and "api.github.com" in url:
        headers["Authorization"] = f"Bearer {token}"
    with urllib.request.urlopen(urllib.request.Request(url, headers=headers), timeout=120) as response:
        return response.read()


def resolve(ref: str) -> str:
    return json.loads(download(f"https://api.github.com/repos/{REPO}/commits/{ref}"))["sha"]


def tar_sha256(commit: str) -> str:
    """Hashes the tar stream, because GitHub does not promise stable gzip bytes."""
    archive = download(f"https://github.com/{REPO}/archive/{commit}.tar.gz")
    return hashlib.sha256(gzip.decompress(archive)).hexdigest()


def main(argv: list) -> int:
    commit = resolve(argv[0] if argv else "master")
    sha256 = tar_sha256(commit)
    LOCK.write_text(f"{HEADER}commit = {commit}\nsha256 = {sha256}\n")
    print(f"pinned {commit}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
