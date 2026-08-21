#!/usr/bin/env python3
"""p2: decode arvo's packed `n` keys and count the dimensions each section varies.

The harness gives a cell exactly one integer parameter (`n`, `SizeSection.n`,
bench-harness/src/config.rs:193). Several arvo bench families need more than
one parameter per cell, so they pack several fields into that one integer and
decode them in Rust. Each shared crate states its own encoding in its module
doc; the decoders below are transcribed from those `pub const fn key_*`
functions, cited per entry.

What this counts, per bench section: how many of the packed fields actually
VARY across the section's declared points. A field that varies is a dimension
of the sweep. A field that is constant across the section is a per-sweep
setting: one value, chosen for the whole sweep, currently expressible nowhere
but inside the integer.

Negative control: a synthetic section whose points differ in exactly one field
must report 1 varying and the rest constant. A section with one point must
report 0 varying and all fields constant. Both printed first.
"""
import sys, tomllib, pathlib, collections

# ---- decoders, transcribed from each shared crate's own key_* functions ----
# arvo/mock/benches/variants/warm-container-shared/src/lib.rs:59,108-126
def dec_warm_container(k):
    return {"W": k // 10_000, "NC": (k // 1_000) % 10, "OP": (k // 100) % 10, "D": k % 100}

# arvo/mock/benches/variants/warm-clamp-shared/src/lib.rs:83,101-115
def dec_warm_clamp(k):
    return {"W": k // 10_000, "NC": (k // 1_000) % 10, "LOG2A": (k // 10) % 100, "OP": k % 10}

# arvo/mock/benches/variants/satfold-shared/src/lib.rs:100,129-140
def dec_satfold(k):
    return {"LI": k // 1_000 - 1, "NC": (k // 100) % 10, "AL": (k // 10) % 10, "OP": k % 10}

# arvo/mock/benches/variants/wide-rung-shared/src/lib.rs:43
def dec_wide_rung(k):
    return {"W": k // 1_000, "NC": (k // 100) % 10, "D": k % 100}

# arvo/mock/benches/variants/bitpack-contend-shared/src/routine.rs:23-25
def dec_contend(k):
    return {"N": k // 10, "T": k % 10}

# which decoder each bench section uses, read off the routine_for_n table in
# arvo/mock/benches/src/main.rs:227-595 (the bridge type names the crate)
SECTION_DECODER = {}
for s in ["warm-container-width-l1", "warm-container-width-l2", "warm-container-density-w13",
          "warm-container-density-w64", "precise-container-width-l1", "warm-elementwise-width-l1",
          "precise-elementwise-width-l1", "warm-affine-collapse-l1", "precise-widening-theorem-l1",
          "warm-affine-density-w13"]:
    SECTION_DECODER[s] = ("Case (warm-container-shared)", dec_warm_container)
for s in ["warm-clamp-arity-w8", "warm-clamp-arity-w13", "warm-clamp-arity-w16", "warm-clamp-arity-w32",
          "warm-clamp-arity-w60", "warm-clamp-arity-w64", "warm-clamp-chain-l1", "warm-clamp-arity-l2"]:
    SECTION_DECODER[s] = ("ClampCase (warm-clamp-shared)", dec_warm_clamp)
for s in ["satfold-length-l1", "satfold-length-l1-wrap", "satfold-align-l1", "satfold-length-dram",
          "satfold-length-dram-long", "satfold-length-dram-wrap", "satfold-const-gate"]:
    SECTION_DECODER[s] = ("SatFoldCase (satfold-shared)", dec_satfold)
for s in ["wide-rung-width-l1", "wide-rung-width-l2", "wide-rung-density-w200",
          "wide-rung-walk-l1", "wide-rung-walk-l2"]:
    SECTION_DECODER[s] = ("WideCase (wide-rung-shared)", dec_wide_rung)
for s in ["bitpack-contention", "bitpack-contend-decode", "bitpack-contend-best",
          "bitpack-write-contend-safe", "bitpack-write-contend-race", "bitpack-wide"]:
    SECTION_DECODER[s] = ("Contend/WriteContend (N*10+T)", dec_contend)


def points_of(sect):
    pts = []
    for s in sect.get("sizes", []):
        pts.append(s["n"] if isinstance(s, dict) else s)
    return pts


def analyse(points, dec):
    fields = collections.defaultdict(set)
    for p in points:
        for k, v in dec(p).items():
            fields[k].add(v)
    varying = {k: sorted(v) for k, v in fields.items() if len(v) > 1}
    constant = {k: next(iter(v)) for k, v in fields.items() if len(v) == 1}
    return varying, constant


def control():
    # one field varies
    v, c = analyse([130003, 130008], dec_warm_container)
    assert list(v) == ["D"] and c == {"W": 13, "NC": 0, "OP": 0}, (v, c)
    # nothing varies
    v2, c2 = analyse([130003], dec_warm_container)
    assert v2 == {} and len(c2) == 4, (v2, c2)
    # two fields vary
    v3, _ = analyse([163841, 10485764], dec_contend)
    assert sorted(v3) == ["N", "T"], v3
    print("control: 1-varying reads", list(v), "| 0-varying reads", v2,
          "| 2-varying reads", sorted(v3), "-> analyser discriminates")


def main(root):
    control()
    doc = tomllib.loads((pathlib.Path(root) / "arvo/mock/benches/bench.toml").read_text())
    benches = doc["bench"]
    packed = plain = 0
    dim_hist = collections.Counter()
    const_hist = collections.Counter()
    print(f"\n{'section':<34} {'bridge':<32} {'pts':>4} varying / constant")
    for name in benches:
        pts = points_of(benches[name])
        if name not in SECTION_DECODER:
            plain += 1
            continue
        packed += 1
        label, dec = SECTION_DECODER[name]
        v, c = analyse(pts, dec)
        dim_hist[len(v)] += 1
        const_hist[len(c)] += 1
        vs = ",".join(f"{k}={len(vals)}" for k, vals in sorted(v.items()))
        cs = ",".join(f"{k}={val}" for k, val in sorted(c.items()))
        print(f"{name:<34} {label:<32} {len(pts):>4} [{vs}] / [{cs}]")
    print(f"\nsections with a packed key: {packed}")
    print(f"sections with a plain point (no packing found): {plain}")
    print("varying-dimension count histogram:", dict(sorted(dim_hist.items())))
    print("constant-field count histogram:   ", dict(sorted(const_hist.items())))
    total_const = sum(k * v for k, v in const_hist.items())
    total_vary = sum(k * v for k, v in dim_hist.items())
    print(f"total per-sweep CONSTANT settings buried in the integer: {total_const}")
    print(f"total per-sweep VARYING axes buried in the integer:      {total_vary}")


if __name__ == "__main__":
    main(sys.argv[1] if len(sys.argv) > 1 else "/Users/orgrinrt/Dev/clause-dev")
