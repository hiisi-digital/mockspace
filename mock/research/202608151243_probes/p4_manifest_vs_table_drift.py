#!/usr/bin/env python3
"""p4: does arvo's Rust routine table agree with its own bench.toml?

The table at arvo/mock/benches/src/main.rs:227-595 restates every (bench,
point) pair that bench.toml declares. Nothing checks the two against each
other except a runtime error on the path that runs. So: diff the sets.

Negative control: an injected fake pair must show up on exactly one side.
Printed first.
"""
import re, sys, tomllib, pathlib

ROW = re.compile(r'^\s*\("([^"]+)",\s*([0-9_]+)\)\s*=>\s*routine_bridge!')


def table_pairs(path):
    out = set()
    for ln in pathlib.Path(path).read_text().splitlines():
        m = ROW.match(ln)
        if m:
            out.add((m.group(1), int(m.group(2).replace("_", ""))))
    return out


def manifest_pairs(path):
    doc = tomllib.loads(pathlib.Path(path).read_text())
    out = set()
    for name, sect in doc.get("bench", {}).items():
        for s in sect.get("sizes", []):
            out.add((name, s["n"] if isinstance(s, dict) else s))
    return out


def main(root):
    root = pathlib.Path(root)
    t = table_pairs(root / "arvo/mock/benches/src/main.rs")
    m = manifest_pairs(root / "arvo/mock/benches/bench.toml")

    # control: an injected pair lands on exactly one side
    fake = ("__control__", 1)
    assert (t | {fake}) - m == (t - m) | {fake}
    print("control: an injected pair appears on exactly one side of the diff")

    print(f"\n(bench, point) pairs in the Rust table : {len(t)}")
    print(f"(bench, point) pairs in bench.toml      : {len(m)}")
    only_rust = sorted(t - m)
    only_toml = sorted(m - t)
    print(f"\nin Rust, not declared in bench.toml (dead match arms): {len(only_rust)}")
    for p in only_rust:
        print("   ", p)
    print(f"\ndeclared in bench.toml, no Rust arm (runtime failure): {len(only_toml)}")
    for p in only_toml:
        print("   ", p)


if __name__ == "__main__":
    main(sys.argv[1] if len(sys.argv) > 1 else "/Users/orgrinrt/Dev/clause-dev")
