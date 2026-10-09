#!/usr/bin/env python3
import argparse
import os
import re
import sys
from dataclasses import dataclass
from typing import Callable, Optional, TextIO

UNIT_NS = {"ps": 1e-3, "ns": 1.0, "µs": 1e3, "us": 1e3, "ms": 1e6, "s": 1e9}
PREFIX = re.compile(r"^((?:[│ ]  )*)[├╰]─ ")
MEDIAN_COLUMN = 2
INDENT_WIDTH = 3


@dataclass(frozen=True)
class Row:
    path: str
    base_ns: Optional[float]
    head_ns: Optional[float]
    ratio: Optional[float]
    regressed: bool


def to_ns(cell: str) -> float:
    value, unit = cell.split()
    if unit not in UNIT_NS:
        raise ValueError(f"unknown divan time unit: {unit!r}")
    return float(value) * UNIT_NS[unit]


def parse(text: str) -> dict[str, float]:
    stack: list[str] = []
    medians: dict[str, float] = {}
    for line in text.splitlines():
        prefix = PREFIX.match(line)
        if prefix is None:
            continue
        depth = len(prefix.group(1)) // INDENT_WIDTH
        cells = [c.strip() for c in line[prefix.end():].split("│")]
        del stack[depth:]
        stack.append(cells[0].split()[0])
        if len(cells) > MEDIAN_COLUMN and cells[MEDIAN_COLUMN]:
            medians["/".join(stack)] = to_ns(cells[MEDIAN_COLUMN])
    return medians


def compare(
    base: dict[str, float], head: dict[str, float], threshold: float
) -> list[Row]:
    def row(path: str) -> Row:
        b, h = base.get(path), head.get(path)
        ratio = h / b if b and h is not None else None
        return Row(path, b, h, ratio, ratio is not None and ratio >= threshold)

    return [row(p) for p in sorted(base.keys() | head.keys())]


def fmt_ns(ns: Optional[float]) -> str:
    return "n/a" if ns is None else f"{ns / 1e6:.3f} ms"


def render(rows: list[Row], threshold: float) -> str:
    lines = [
        "### Synth benchmarks: head vs merge base (median)",
        "",
        "| bench | base | head | ratio |",
        "|---|---|---|---|",
    ]
    for r in rows:
        ratio = "n/a" if r.ratio is None else f"{r.ratio:.2f}x"
        flag = " :warning:" if r.regressed else ""
        lines.append(
            f"| `{r.path}` | {fmt_ns(r.base_ns)} | {fmt_ns(r.head_ns)} | {ratio}{flag} |"
        )
    lines.append("")
    lines.append(
        f"Warning threshold: {threshold:.1f}x. Shared-runner noise; advisory only."
    )
    return "\n".join(lines) + "\n"


def main(
    argv: list[str],
    read: Callable[[str], str] = lambda p: open(p, encoding="utf-8").read(),
    out: TextIO = sys.stdout,
) -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("base")
    ap.add_argument("head")
    ap.add_argument("--threshold", type=float, default=2.0)
    args = ap.parse_args(argv)

    base, head = parse(read(args.base)), parse(read(args.head))
    if not base or not head:
        print(
            f"::error::no benchmark rows parsed (base={len(base)}, head={len(head)}); "
            "divan output format may have changed or a bench panicked",
            file=out,
        )
        return 2

    rows = compare(base, head, args.threshold)
    table = render(rows, args.threshold)
    out.write(table)
    for r in rows:
        if r.regressed:
            print(
                f"::warning::{r.path} is {r.ratio:.2f}x slower than merge base "
                f"({fmt_ns(r.base_ns)} -> {fmt_ns(r.head_ns)})",
                file=out,
            )
    summary = os.environ.get("GITHUB_STEP_SUMMARY")
    if summary:
        with open(summary, "a", encoding="utf-8") as f:
            f.write(table)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
