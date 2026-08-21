#!/usr/bin/env python3
"""F: render arvo's warm-container family as the sweep table it already is.

Decodes each section's packed points with the family's own encoding
(arvo/mock/benches/variants/warm-container-shared/src/lib.rs:59,108-126) and
prints, per section, which axis varies and which fields are held. This is the
worked example the design is written against; nothing here is invented.

Control: a synthetic section with two points differing in one field must show
that field varying and the rest held.
"""
import tomllib, pathlib, collections

def dec(k):
    return {"w": k // 10_000, "nc": (k // 1_000) % 10, "op": (k // 100) % 10, "d": k % 100}

OP_NAMES = {0: "wrap-reduce", 1: "sat-reduce", 2: "wrap-elementwise",
            3: "sat-elementwise", 4: "wrap-affine", 5: "sat-widen"}
NC_NAMES = {0: "8192 (L1-resident)", 1: "1048576 (L2/DRAM)"}

FAMILY = ["warm-container-width-l1", "warm-container-width-l2",
          "warm-container-density-w13", "warm-container-density-w64",
          "precise-container-width-l1", "warm-elementwise-width-l1",
          "precise-elementwise-width-l1", "warm-affine-collapse-l1",
          "precise-widening-theorem-l1", "warm-affine-density-w13"]

def control():
    f = collections.defaultdict(set)
    for p in (130003, 130008):
        for k, v in dec(p).items():
            f[k].add(v)
    vary = [k for k, v in f.items() if len(v) > 1]
    assert vary == ["d"], vary
    print("control: two points differing in D report exactly ['d'] varying\n")

def main():
    control()
    doc = tomllib.loads(pathlib.Path(
        "/Users/orgrinrt/Dev/clause-dev/arvo/mock/benches/bench.toml").read_text())["bench"]
    print(f"{'sweep':<30} {'axis':<6} {'values':<34} held")
    for name in FAMILY:
        pts = [s["n"] if isinstance(s, dict) else s for s in doc[name]["sizes"]]
        f = collections.defaultdict(set)
        for p in pts:
            for k, v in dec(p).items():
                f[k].add(v)
        vary = {k: sorted(v) for k, v in f.items() if len(v) > 1}
        held = {k: next(iter(v)) for k, v in f.items() if len(v) == 1}
        ax = ",".join(vary) or "-"
        vals = ";".join(str(v) for v in vary.values()) or "-"
        heldstr = ", ".join(
            f"{k}={OP_NAMES[v] if k=='op' else (NC_NAMES[v] if k=='nc' else v)}"
            for k, v in sorted(held.items()))
        print(f"{name.replace('warm-container-','').replace('precise-','P-'):<30} {ax:<6} {vals:<34} {heldstr}")
    arms = doc[FAMILY[0]]["sizes"][0].get("variants", []) or doc[FAMILY[0]].get("variants", [])
    print(f"\narm list on the first sweep ({len(arms)}): {[a.split('/')[-1] for a in arms]}")
    same = all(
        [v.get("variants", []) for v in doc[n]["sizes"]] ==
        [v.get("variants", []) for v in doc[FAMILY[0]]["sizes"]][:len(doc[n]["sizes"])]
        for n in FAMILY[:2])
    print("first two sweeps share their arm list:", same)

main()
