#!/usr/bin/env python3
"""Fails when a part of the workspace falls below its line-coverage floor.

    cargo llvm-cov report --summary-only > coverage.txt
    python3 scripts/coverage-floors.py coverage.txt

`cargo llvm-cov --fail-under-lines` only checks the total, where the window's
views (gitlab-tracker/src/gui/, drawing code covered by the headless render
tests) would hide a drop in the logic. So each library crate, and the binary
outside gui/, has a floor of its own.

The floors only go up: set each one about a point under the measured value,
and raise it when coverage grows (the table printed here gives the figures).
"""

import sys
from collections import defaultdict

# Line coverage, in percent. Measured on 2026-11-24: core 84.7, gitlab 90.8,
# redmine 91.5, stats 95.4, store 90.9, binary outside gui/ 74.9.
FLOORS = {
    "gitlab-tracker-core": 83.5,
    "gitlab-tracker-gitlab": 89.5,
    "gitlab-tracker-redmine": 90.5,
    "gitlab-tracker-stats": 94.0,
    "gitlab-tracker-store": 89.5,
    "gitlab-tracker (outside gui/)": 74.0,
}


def part(path):
    crate = path.split("/")[0]
    if crate != "gitlab-tracker":
        return crate
    return None if "/src/gui/" in path else "gitlab-tracker (outside gui/)"


def main():
    lines = defaultdict(lambda: [0, 0])  # part → [lines, missed]
    with open(sys.argv[1]) as report:
        for row in report:
            cols = row.split()
            # Filename, regions (3 cols), functions (3), lines (3), branches (3).
            if len(cols) < 10 or not cols[0].endswith(".rs"):
                continue
            name = part(cols[0])
            if name:
                lines[name][0] += int(cols[7])
                lines[name][1] += int(cols[8])

    failed = []
    print(f"{'part':32} {'lines':>7} {'covered':>8} {'floor':>6}")
    for name, floor in FLOORS.items():
        total, missed = lines.get(name, (0, 0))
        if total == 0:
            failed.append(f"{name}: no line in the report")
            continue
        covered = 100 * (total - missed) / total
        mark = "" if covered >= floor else "  ← below its floor"
        print(f"{name:32} {total:7} {covered:7.1f}% {floor:5.1f}%{mark}")
        if covered < floor:
            failed.append(f"{name}: {covered:.1f}% < {floor}%")
    if failed:
        sys.exit("Coverage under its floor (add tests, or explain the drop):\n  "
                 + "\n  ".join(failed))


if __name__ == "__main__":
    main()
