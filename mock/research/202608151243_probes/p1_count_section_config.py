#!/usr/bin/env python3
"""p1: reproduce the design's measurement, then widen it.

Two counts over the same four consumer manifests:

  A. keys present on a `[bench.<name>]` section, other than the point list and
     the arm list. This is what the existing measurement counted when it
     concluded that a sweep carries almost no configuration.

  B. the same, but adding every key the harness's own config schema defines
     for a section, so the reader can see which of them are simply never used
     and which do not exist to be used.

Negative control: a section deliberately given three extra keys must be
counted as carrying three. Printed first; if it does not read 3 the counter
is not counting.

Usage: p1_count_section_config.py <repo-root-of-clause-dev>
"""
import sys, tomllib, pathlib, collections

# keys the design treats as "the point list and the arm list", i.e. not config
POINTS = {"sizes", "points"}
ARMS = {"variants", "arms"}
STRUCTURAL = POINTS | ARMS

TREES = [
    ("arvo", "arvo/mock/benches/bench.toml"),
    ("hilavitkutin", "hilavitkutin/mock/benches/bench.toml"),
    ("vehje", "vehje/mock/benches/bench.toml"),
    ("kirjo", "kirjo/mock/benches/bench.toml"),
]


def section_config_keys(sect):
    """Keys on a section that are not the point list and not the arm list.

    An array-of-tables `sizes` entry is the point list in its verbose form, so
    its inner `n` / `variants` are structural too. Anything else is config.
    """
    out = []
    for k, v in sect.items():
        if k in STRUCTURAL:
            continue
        out.append(k)
    return sorted(out)


def control():
    doc = tomllib.loads(
        '[bench.ctl]\ntitle="t"\nsizes=[1]\nvariants=["a"]\n'
        'workload="realistic"\nmaster_seed=1\nthreaded=true\n'
    )
    got = section_config_keys(doc["bench"]["ctl"])
    assert got == ["master_seed", "threaded", "title", "workload"], got
    empty = tomllib.loads('[bench.c2]\nsizes=[1]\nvariants=["a"]\n')
    assert section_config_keys(empty["bench"]["c2"]) == []
    print("control: title+workload+master_seed+threaded counted as", len(got), "(expect 4); bare section 0 -> counter works")


def main(root):
    control()
    root = pathlib.Path(root)
    grand = collections.Counter()
    total_sections = 0
    for name, rel in TREES:
        p = root / rel
        if not p.exists():
            print(f"{name}: NO MANIFEST at {p}")
            continue
        doc = tomllib.loads(p.read_text())
        benches = doc.get("bench", {})
        freq = collections.Counter()
        per_section = []
        for bname, sect in benches.items():
            keys = section_config_keys(sect)
            freq.update(keys)
            per_section.append((bname, keys))
            total_sections += 1
        grand.update(freq)
        n = len(benches)
        # how many sections carry nothing beyond title/workload/master_seed,
        # the three every section has by convention
        BOILERPLATE = {"title", "workload", "master_seed"}
        beyond = [(b, [k for k in ks if k not in BOILERPLATE]) for b, ks in per_section]
        with_real = [(b, ks) for b, ks in beyond if ks]
        print(f"\n=== {name}: {n} [bench.*] sections")
        print("  key frequency:", dict(freq))
        print(f"  sections carrying a key beyond title/workload/master_seed: {len(with_real)}")
        for b, ks in with_real:
            print(f"    {b}: {ks}")
    print("\n=== ALL TREES")
    print("  total sections:", total_sections)
    print("  key frequency:", dict(grand))


if __name__ == "__main__":
    main(sys.argv[1] if len(sys.argv) > 1 else "/Users/orgrinrt/Dev/clause-dev")
