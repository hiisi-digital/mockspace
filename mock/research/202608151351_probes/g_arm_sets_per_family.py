#!/usr/bin/env python3
"""G: within each family the design proposes as one bench, do its sweeps agree
on an arm set?

The design's section 5 says a sweep "overrides only what differs, which the
survey measured as rare (257 of 258 sweeps share their bench's arm set)". That
figure is about variation WITHIN a section across its points. This one asks the
different question the new vocabulary creates: across the sweeps that become one
bench, how many distinct arm sets are there?

Families are derived the way the design derived them: connected components of
the graph whose nodes are sections and whose edges are a shared arm.

Control: two synthetic sections sharing an arm must land in one component; two
sharing none must not.
"""
import tomllib, pathlib, collections

def arms_of(sect):
    sets = set()
    for s in sect.get("sizes", []):
        own = s.get("variants", []) if isinstance(s, dict) else []
        sets.add(tuple(sorted(own or sect.get("variants", []))))
    return sets

def components(benches):
    by_arm = collections.defaultdict(set)
    for name, sect in benches.items():
        for s in arms_of(sect):
            for a in s:
                by_arm[a].add(name)
    parent = {n: n for n in benches}
    def find(x):
        while parent[x] != x:
            parent[x] = parent[parent[x]]; x = parent[x]
        return x
    def union(a, b):
        ra, rb = find(a), find(b)
        if ra != rb: parent[ra] = rb
    for owners in by_arm.values():
        owners = list(owners)
        for o in owners[1:]:
            union(owners[0], o)
    comp = collections.defaultdict(list)
    for n in benches:
        comp[find(n)].append(n)
    return list(comp.values())

def control():
    doc = tomllib.loads(
        '[bench.a]\ntitle="t"\nworkload="w"\nvariants=["x","y"]\nsizes=[1]\n'
        '[bench.b]\ntitle="t"\nworkload="w"\nvariants=["y","z"]\nsizes=[1]\n'
        '[bench.c]\ntitle="t"\nworkload="w"\nvariants=["q"]\nsizes=[1]\n')["bench"]
    cs = sorted([sorted(c) for c in components(doc)])
    assert cs == [["a", "b"], ["c"]], cs
    print("control: sections sharing an arm form one component, others stay apart\n")

def main():
    control()
    doc = tomllib.loads(pathlib.Path(
        "/Users/orgrinrt/Dev/clause-dev/arvo/mock/benches/bench.toml").read_text())["bench"]
    comps = sorted(components(doc), key=lambda c: -len(c))
    multi = [c for c in comps if len(c) > 1]
    print(f"sections: {len(doc)}   families (components): {len(comps)}   "
          f"multi-section families: {len(multi)}   singletons: {len(comps)-len(multi)}")
    total_sweeps = over = 0
    print(f"\n{'family (by largest member)':<34} {'sweeps':>6} {'distinct arm sets':>18}")
    for c in comps:
        sets = set()
        for n in c:
            for s in arms_of(doc[n]):
                sets.add(s)
        total_sweeps += len(c)
        if len(sets) > 1:
            over += len(c)
        if len(c) > 1:
            print(f"{sorted(c)[0]:<34} {len(c):>6} {len(sets):>18}")
    print(f"\ntotal sweeps: {total_sweeps}")
    print(f"sweeps sitting in a family whose arm sets are NOT all identical: {over}")

main()
