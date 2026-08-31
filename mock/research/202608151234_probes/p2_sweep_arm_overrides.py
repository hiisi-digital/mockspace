#!/usr/bin/env python3
"""How much per-sweep configuration does the proposed schema actually need?

Usage: p2_sweep_arm_overrides.py <arvo/mock/benches>

Two different questions live in "sweeps share their bench's arm set":

  (a) within one section, do the points share an arm set?
  (b) within one bench (a family of sections sharing arms), do the
      sections share an arm set?

(a) is the point-override question. (b) is the sweep-override question,
and it is the one the proposed [sweep.*] schema turns on. This prints both.

Families are derived by union-find over shared arms, which also reproduces
the family/singleton structure independently.

Negative control: a tree where every section had a unique arm set would
report 0 families and 0 overrides; a tree where all sections were identical
would report 1 family and 0 overrides. Both extremes are distinguishable
from the printed output, so a bug cannot read as a clean result.
"""
import re, sys, collections, pathlib

root = pathlib.Path(sys.argv[1] if len(sys.argv) > 1 else ".")
toml = (root / "bench.toml").read_text()

rows = collections.defaultdict(list)
# NOTE: an earlier version of this probe required `variants` to follow `n`
# immediately. That silently skipped the 4 rows in arvo that carry a comment
# between them, and those 4 are exactly the rows with a per-point arm
# override, so the probe reported "no row varies" by excluding every row that
# could have varied. Parse the whole block instead.
for m in re.finditer(r'\[\[bench\.([^\]]+?)\.sizes\]\](.*?)(?=\n\[|\Z)', toml, re.S):
    body = m.group(2)
    vm = re.search(r'variants = \[(.*?)\]', body, re.S)
    if not vm:
        raise SystemExit(f"size row in {m.group(1)} has no variants list; parse is wrong")
    arms = tuple(sorted(a.split('/')[-1] for a in re.findall(r'"([^"]+)"', vm.group(1))))
    rows[m.group(1)].append(arms)

parsed = sum(len(v) for v in rows.values())
varying = [k for k, v in rows.items() if len(set(v)) > 1]
print(f"sections parsed                        : {len(rows)}")
print(f"size rows parsed                       : {parsed}")
print(f"(a) rows whose arms differ from their")
print(f"    section's first row                : {sum(len(set(v)) - 1 for v in rows.values())}"
      f"  in {len(varying)} section(s) {varying}")

sec = {k: v[0] for k, v in rows.items()}
fam = collections.defaultdict(set)
for k, arms in sec.items():
    for a in arms:
        fam[a].add(k)
parent = {k: k for k in sec}
def find(x):
    while parent[x] != x:
        parent[x] = parent[parent[x]]; x = parent[x]
    return x
for a, s in fam.items():
    s = list(s)
    for x in s[1:]:
        ra, rb = find(s[0]), find(x)
        if ra != rb: parent[ra] = rb
groups = collections.defaultdict(list)
for k in sec: groups[find(k)].append(k)

shared = sum(1 for a, s in fam.items() if len(s) > 1)
fams = [g for g in groups.values() if len(g) > 1]
print(f"\narms appearing in >1 section           : {shared}")
print(f"families (>1 section)                  : {len(fams)}")
print(f"singletons                             : {sum(1 for g in groups.values() if len(g) == 1)}")

need = 0
for g in groups.values():
    c = collections.Counter(sec[k] for k in g)
    need += len(g) - c.most_common(1)[0][1]
print(f"\n(b) sweeps needing an `arms` override  : {need} of {len(sec)}"
      f"  ({100*need/len(sec):.0f}%)")
for g in sorted(fams, key=len, reverse=True):
    c = collections.Counter(sec[k] for k in g)
    print(f"  family of {len(g):>2} ({sorted(g)[0]}...): "
          f"{len(c)} distinct arm set(s), {len(g)-c.most_common(1)[0][1]} override(s)")
