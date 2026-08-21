#!/usr/bin/env python3
"""H: how many places is arvo's point set written, and do the copies agree?

Four locations carry the same integers:
  1. bench.toml `[[bench.<n>.sizes]] n = ...`
  2. src/main.rs `("<bench>", <point>) => routine_bridge!(...)`
  3. each arm crate's `#[bench_variant(..., sizes = [...])]`
  4. three shared crates' `pub const ALL_KEYS`

Counted, and the three families that have an ALL_KEYS table are diffed against
the manifest so a fourth copy is shown to agree rather than assumed to.

Controls: a set difference reports a removed element, and a length difference
reports one. Printed first.
"""
import re, tomllib, pathlib, collections

ROOT = pathlib.Path("/Users/orgrinrt/Dev/clause-dev/arvo/mock/benches")

FAMILIES = {
    "warm-container-shared": [
        "warm-container-width-l1", "warm-container-width-l2",
        "warm-container-density-w13", "warm-container-density-w64",
        "precise-container-width-l1", "warm-elementwise-width-l1",
        "precise-elementwise-width-l1", "warm-affine-collapse-l1",
        "precise-widening-theorem-l1", "warm-affine-density-w13"],
    "warm-clamp-shared": [
        "warm-clamp-arity-w8", "warm-clamp-arity-w13", "warm-clamp-arity-w16",
        "warm-clamp-arity-w32", "warm-clamp-arity-w60", "warm-clamp-arity-w64",
        "warm-clamp-chain-l1", "warm-clamp-arity-l2"],
    "wide-rung-shared": [
        "wide-rung-width-l1", "wide-rung-width-l2", "wide-rung-density-w200",
        "wide-rung-walk-l1", "wide-rung-walk-l2"],
}

INTS = r'\d[\d_]*'


def ints(text):
    return [int(x.replace("_", "")) for x in re.findall(INTS, text)]


def control():
    a = {1, 2, 3}
    assert a - (a - {3}) == {3}
    assert len([1, 2, 3]) - len([1, 2]) == 1
    print("control: a set difference reports the removed element and a length "
          "difference reports one\n")


def main():
    control()
    doc = tomllib.loads((ROOT / "bench.toml").read_text())["bench"]

    manifest_rows = sum(len(s["sizes"]) for s in doc.values())
    table_rows = len(re.findall(
        r'^\s*\("[^"]+",\s*[0-9_]+\)\s*=>', (ROOT / "src/main.rs").read_text(), re.M))

    attr = {}
    for lib in sorted((ROOT / "variants").glob("*/src/lib.rs")):
        m = re.search(r'#\[bench_variant\((.*?)\)\]', lib.read_text(), re.S)
        if not m:
            continue
        sz = re.search(r'sizes\s*=\s*\[(.*?)\]', m.group(1), re.S)
        if sz:
            attr[lib.parts[-3]] = len(ints(sz.group(1)))

    allkeys = {}
    for lib in sorted((ROOT / "variants").glob("*/src/lib.rs")):
        m = re.search(r'pub const ALL_KEYS[^=]*=\s*&?\[(.*?)\];', lib.read_text(), re.S)
        if m:
            allkeys[lib.parts[-3]] = set(ints(re.sub(r'//[^\n]*', '', m.group(1))))

    print("integer point literals maintained by hand, by location")
    print(f"  1. bench.toml [[sizes]] rows                  : {manifest_rows}")
    print(f"  2. src/main.rs routine-table rows             : {table_rows}")
    print(f"  3. #[bench_variant(sizes=[..])] over {len(attr):>3} arms  : {sum(attr.values())}")
    print(f"  4. ALL_KEYS in {len(allkeys)} shared crates             : "
          f"{sum(len(v) for v in allkeys.values())}")
    print(f"  TOTAL                                         : "
          f"{manifest_rows + table_rows + sum(attr.values()) + sum(len(v) for v in allkeys.values())}")
    print(f"  distinct (bench, point) pairs they describe   : {manifest_rows}")

    print("\ndo copies 1 and 4 agree, per family with an ALL_KEYS table?")
    for crate, sections in FAMILIES.items():
        man = set()
        for s in sections:
            for e in doc[s]["sizes"]:
                man.add(e["n"] if isinstance(e, dict) else e)
        ak = allkeys.get(crate)
        if ak is None:
            print(f"  {crate:<24} no ALL_KEYS table")
            continue
        print(f"  {crate:<24} manifest {len(man):>3}  ALL_KEYS {len(ak):>3}  "
              f"only-in-ALL_KEYS {sorted(ak - man)}  only-in-manifest {sorted(man - ak)}")

    print("\nlargest per-arm sizes attributes (each is the whole family's point set):")
    for k, v in sorted(attr.items(), key=lambda kv: -kv[1])[:6]:
        print(f"  {k}: {v}")


if __name__ == "__main__":
    main()
