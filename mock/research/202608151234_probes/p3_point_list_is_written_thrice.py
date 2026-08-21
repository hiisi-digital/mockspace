#!/usr/bin/env python3
"""How many times is one bench's point list written down?

Usage: p3_point_list_is_written_thrice.py <arvo/mock/benches> [bench-name]

For a single bench, counts the restatements of its point list across the
three places it lives: each arm crate's #[bench_variant(... sizes = [...])]
attribute, the manifest's size rows, and the driver's match arms.

Negative control: a bench whose arms do not carry the list would report 0
for the arm column, which is distinguishable from a parse failure because
the arm-crate count is printed separately from the match count.
"""
import re, sys, pathlib

root = pathlib.Path(sys.argv[1] if len(sys.argv) > 1 else ".")
bench = sys.argv[2] if len(sys.argv) > 2 else "bitpack-carrier-width"

toml = (root / "bench.toml").read_text()
sec = re.search(rf'\[bench\.{re.escape(bench)}\](.*?)(?=\n\[bench\.|\Z)', toml, re.S)
if not sec:
    raise SystemExit(f"no [bench.{bench}] section")
body = sec.group(1)
points = re.findall(r'^n = (\d+)', body, re.M)
arms = sorted({a.split('/')[-1] for a in re.findall(r'"([^"]+)"', body)
               if '/' in a})
literal = ", ".join(points)

arm_dirs = list((root / "variants").iterdir()) if (root / "variants").is_dir() else []
arm_hits = [d.name for d in arm_dirs
            if (d / "src/lib.rs").is_file()
            and literal in (d / "src/lib.rs").read_text()]

main = (root / "src/main.rs").read_text()
matches = re.findall(rf'\("{re.escape(bench)}", (\d+)\) => routine_bridge!\((\w+)', main)

print(f"bench                         : {bench}")
print(f"points                        : {len(points)}  [{literal}]")
print(f"arms in manifest              : {len(arms)}")
print(f"arm crates restating the list : {len(arm_hits)}  {arm_hits}")
print(f"manifest rows                 : {len(points)}")
print(f"driver match arms             : {len(matches)}")
print(f"bridge types in driver        : {sorted({m[1] for m in matches})}")
print(f"\ntotal restatements of the same {len(points)} integers: "
      f"{len(arm_hits)} + 1 + {len(points)} = {len(arm_hits) + 1 + len(points)}")
