#!/usr/bin/env python3
"""p5: reproduce the committed claim closest to the one under doubt.

The design says (202608150809_bench-vocabulary-and-consolidation-design.md:430-431):
"257 of 258 sweeps share their bench's arm set", sourced to the survey's
"of the 258 benches using this form, exactly one (in arvo) has a variant set
that actually differs between its sizes".

Reproduced here over the four consumer manifests: for each bench declared in
the array-of-tables form, is the per-size arm list constant across its sizes?

Negative control: a synthetic bench whose two size blocks list different arms
must be counted as differing; one whose blocks match must not.
"""
import sys, tomllib, pathlib

TREES = [
    ("arvo", "arvo/mock/benches/bench.toml"),
    ("hilavitkutin", "hilavitkutin/mock/benches/bench.toml"),
    ("vehje", "vehje/mock/benches/bench.toml"),
    ("kirjo", "kirjo/mock/benches/bench.toml"),
]


def per_size_arm_sets(sect):
    """The arm list each size block declares, or None where it inherits."""
    out = []
    for s in sect.get("sizes", []):
        if isinstance(s, dict):
            out.append(tuple(s.get("variants", [])) or None)
        else:
            out.append(None)
    return out


def control():
    same = tomllib.loads(
        '[[bench.x.sizes]]\nn=1\nvariants=["a","b"]\n'
        '[[bench.x.sizes]]\nn=2\nvariants=["a","b"]\n'
    )["bench"]["x"]
    diff = tomllib.loads(
        '[[bench.y.sizes]]\nn=1\nvariants=["a","b"]\n'
        '[[bench.y.sizes]]\nn=2\nvariants=["a"]\n'
    )["bench"]["y"]
    assert len(set(per_size_arm_sets(same))) == 1
    assert len(set(per_size_arm_sets(diff))) == 2
    print("control: identical blocks read as 1 distinct arm set, differing blocks as 2")


def main(root):
    control()
    root = pathlib.Path(root)
    aot = 0
    varying = []
    for name, rel in TREES:
        p = root / rel
        if not p.exists():
            continue
        doc = tomllib.loads(p.read_text())
        for bname, sect in doc.get("bench", {}).items():
            sets = per_size_arm_sets(sect)
            # array-of-tables form: at least one size block declared its own arms
            if not any(s is not None for s in sets):
                continue
            aot += 1
            if len(set(sets)) > 1:
                varying.append((name, bname, len(set(sets))))
    print(f"\nbenches using the per-size arm-list form: {aot}")
    print(f"of those, benches whose arm set actually differs between sizes: {len(varying)}")
    for v in varying:
        print("   ", v)


if __name__ == "__main__":
    main(sys.argv[1] if len(sys.argv) > 1 else "/Users/orgrinrt/Dev/clause-dev")
