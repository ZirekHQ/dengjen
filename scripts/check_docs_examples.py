#!/usr/bin/env python3
import json
import py_compile
import re
import subprocess
import sys
import tempfile
from pathlib import Path

INCLUDE = re.compile(r"^include::example\$([^\[]+)\[(.*)\]\s*$", re.MULTILINE)
TAG_ATTR = re.compile(r"tags?=([^,\]]+)")
JSON_BLOCK = re.compile(r"^\[source,json(?:,[^\]]*)?\]\n----\n(.*?)\n----$", re.MULTILINE | re.DOTALL)
PAGES = Path("docs/modules/ROOT/pages")
EXAMPLES_LINKS = Path("docs/modules/ROOT/examples")


def check_python(root):
    problems = []
    with tempfile.TemporaryDirectory() as out:
        for path in sorted((root / "examples").rglob("*.py")):
            try:
                py_compile.compile(str(path), cfile=str(Path(out) / "x.pyc"), doraise=True)
            except py_compile.PyCompileError as error:
                problems.append(f"{path.relative_to(root)}: {error.msg.strip()}")
    return problems


def check_shell(root):
    problems = []
    for path in sorted((root / "examples").rglob("*.sh")):
        result = subprocess.run(["bash", "-n", str(path)], capture_output=True, text=True)
        if result.returncode != 0:
            problems.append(f"{path.relative_to(root)}: {result.stderr.strip()}")
    return problems


def tags_in(attributes):
    found = TAG_ATTR.search(attributes)
    return [t for t in found.group(1).split(";") if t] if found else []


def missing_tags(source, tags):
    return [t for t in tags if f"tag::{t}[]" not in source or f"end::{t}[]" not in source]


def check_page_includes(root, page):
    problems = []
    for target, attributes in INCLUDE.findall(page.read_text()):
        file = root / EXAMPLES_LINKS / target
        if not file.is_file():
            problems.append(f"{page.relative_to(root)}: include target `{target}` does not exist")
            continue
        for tag in missing_tags(file.read_text(), tags_in(attributes)):
            problems.append(f"{page.relative_to(root)}: `{target}` lacks tag::{tag}[] / end::{tag}[]")
    return problems


def check_includes(root):
    return [p for page in sorted((root / PAGES).rglob("*.adoc")) for p in check_page_includes(root, page)]


def check_symlinks(root):
    problems = []
    for entry in sorted((root / EXAMPLES_LINKS).glob("*")):
        target = entry.resolve()
        inside = (root / "examples").resolve() in target.parents or target == (root / "examples").resolve()
        if not entry.is_symlink() or not target.exists() or not inside:
            problems.append(f"{entry.relative_to(root)}: must be a symlink to a directory under examples/")
    return problems


def check_json_blocks(root):
    problems = []
    for page in sorted((root / PAGES).rglob("*.adoc")):
        for block in JSON_BLOCK.findall(page.read_text()):
            try:
                json.loads(block)
            except json.JSONDecodeError as error:
                problems.append(f"{page.relative_to(root)}: invalid JSON block ({error})")
    return problems


def check(root):
    root = Path(root)
    return (
        check_python(root)
        + check_shell(root)
        + check_includes(root)
        + check_symlinks(root)
        + check_json_blocks(root)
    )


def main():
    problems = check(Path(__file__).resolve().parent.parent)
    for problem in problems:
        print(problem)
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main())
