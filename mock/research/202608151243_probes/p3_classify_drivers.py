#!/usr/bin/env python3
"""p3: classify the two long consumer drivers line by line.

Categories, per the question asked:
  loop        the hand-rolled manifest loop and worker the library driver subsumes
  per_bench   a statement whose only datum is a per-bench choice
  per_point   a statement whose only datum is a per-point choice
  per_arm     a statement whose only datum is a per-arm choice
  oneoff      genuinely consumer-specific behaviour with no config shape
  prose       comment or blank

For arvo the table rows are `("bench", point) => routine_bridge!(Ty<K>)`, which
carry both a per-bench datum (the bridge type) and a per-point datum (the const
argument), so they are counted in a joint bucket and split by how many distinct
types each bench uses.

Negative control: a synthetic file with one known row of each shape must be
classified as that shape. Printed first.
"""
import re, sys, pathlib, collections

ROW = re.compile(r'^\s*\("([^"]+)",\s*([0-9_]+)\)\s*=>\s*routine_bridge!\(([A-Za-z0-9_]+)\s*<')
ROW_BYTE = re.compile(r'^\s*([0-9_]+)\s*=>\s*routine_bridge!')


def control():
    sample = [
        '        ("warm-container-width-l1", 80003) => routine_bridge!(Case<80003>),',
        '            64 => routine_bridge!(ByteRoutine<64, 8, true>),',
        '        // a comment',
        '',
    ]
    assert ROW.match(sample[0]).groups() == ("warm-container-width-l1", "80003", "Case")
    assert ROW.match(sample[1]) is None and ROW_BYTE.match(sample[1])
    assert ROW.match(sample[2]) is None and ROW_BYTE.match(sample[2]) is None
    print("control: keyed row parsed as", ROW.match(sample[0]).groups(),
          "| byte row matched by the byte pattern only | comment matched by neither")


def classify(path, table_span):
    lines = pathlib.Path(path).read_text().splitlines()
    lo, hi = table_span
    counts = collections.Counter()
    rows = []
    for i, ln in enumerate(lines, 1):
        s = ln.strip()
        if not s or s.startswith("//"):
            counts["prose"] += 1
            continue
        if lo <= i <= hi:
            m = ROW.match(ln)
            if m:
                counts["table_row"] += 1
                rows.append(m.groups())
                continue
            if ROW_BYTE.match(ln):
                counts["table_row_byte"] += 1
                continue
            counts["table_other"] += 1
            continue
        counts["loop_or_other_code"] += 1
    return counts, rows, len(lines)


def main(root):
    control()
    root = pathlib.Path(root)

    print("\n=== arvo/mock/benches/src/main.rs")
    c, rows, total = classify(root / "arvo/mock/benches/src/main.rs", (227, 595))
    print("  total lines:", total, dict(c))
    by_bench = collections.defaultdict(set)
    for b, n, ty in rows:
        by_bench[b].add(ty)
    print(f"  keyed table rows: {len(rows)} over {len(by_bench)} bench names")
    multi = {b: t for b, t in by_bench.items() if len(t) > 1}
    print(f"  bench names needing more than one bridge type: {len(multi)} {multi}")
    print("  distinct bridge types used:", len({ty for _, _, ty in rows}))
    print("  => information content of the table: one type per bench name"
          f" ({len(by_bench)} data) plus one const argument per row ({len(rows)} data)")

    print("\n=== hilavitkutin/mock/benches/src/main.rs")
    c2, rows2, total2 = classify(root / "hilavitkutin/mock/benches/src/main.rs", (211, 250))
    print("  total lines:", total2, dict(c2))
    src = (root / "hilavitkutin/mock/benches/src/main.rs").read_text()
    m = re.search(r'let may_differ = matches!\(\s*bench_name,\s*(.*?)\s*\);', src, re.S)
    names = re.findall(r'"([^"]+)"', m.group(1)) if m else []
    print(f"  may_differ names hardcoded in Rust: {len(names)}")
    print("   ", names)


if __name__ == "__main__":
    main(sys.argv[1] if len(sys.argv) > 1 else "/Users/orgrinrt/Dev/clause-dev")
