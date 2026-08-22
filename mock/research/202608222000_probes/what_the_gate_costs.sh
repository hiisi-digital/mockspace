#!/usr/bin/env bash
# What a gate run actually costs, and where. Ad-hoc quick spike, NOT a bench:
# no harness, no arms, no competitors, no artifact trail. It answers "is this
# worth optimising at all", nothing finer.
#
#   ./what_the_gate_costs.sh <path-to-a-mockspace-project>
set -euo pipefail
PROJ="${1:?give me a project root with a mock/ in it}"
export CARGO_BUILD_JOBS=2

echo "== warm gate, whole thing, 3 runs =="
# NOTE: `cd` into the project. The first version of this ran in whatever cwd
# the caller happened to be in, so the number it produced was the denominator
# behind "30ms is 3% of a gate" and was not reproducible from the script.
for _ in 1 2 3; do
    ( cd "$PROJ/mock" && /usr/bin/time -p nice -n 10 cargo mock check >/dev/null ) 2>/tmp/g.$$ || true
    grep '^real' /tmp/g.$$
done < /dev/null

echo "== warm cargo spawn alone, 3 runs =="
GEN="$PROJ/mock/target/mockspace-lints"
for _ in 1 2 3; do
    /usr/bin/time -p nice -n 10 cargo build --release \
        --manifest-path "$GEN/Cargo.toml" \
        --message-format json-render-diagnostics >/dev/null 2>/tmp/c.$$ || true
    grep '^real' /tmp/c.$$
done < /dev/null

echo "== cold: fresh tree, empty cache =="
T=$(mktemp -d); touch "$T/.metadata_never_index"
rsync -a --exclude target --exclude .git "$PROJ/" "$T/tree/"
git -C "$T/tree" init -q .
( cd "$T/tree/mock" && /usr/bin/time -p env CARGO_TARGET_DIR="$T/cold" \
    nice -n 10 cargo mock check >/dev/null 2>"$T/cold.txt" || true )
grep '^real' "$T/cold.txt"
echo "crates compiled: $(grep -cE '^ +Compiling' "$T/cold.txt" || true)"
echo "files in deps/:  $(ls "$T/cold/release/deps" 2>/dev/null | wc -l | tr -d ' ')"
echo "cache size:      $(du -sh "$T/cold" | cut -f1)"
rm -rf "$T"
