#!/usr/bin/env bash
# Probe: does a shared `--target-dir` still share when the arms are
# STANDALONE packages built by `--manifest-path`, which is what the tool
# actually does?
#
# WHY THIS EXISTS. The sibling derivation's probe 04 establishes that one
# shared target directory shares an artifact between arms whose feature
# resolution matches, and separates arms whose resolution differs. It
# builds its arms with `cargo build -p arm-a` inside one workspace.
#
# The tool does neither of those things. `cargo_build_at`
# (`src/bench.rs:508-534`, merged to dev at 93f51bf) builds
# `--manifest-path <arm>/Cargo.toml`, and every consumer arm carries its
# own `[workspace]` marker or sits under an excluded path, so each is a
# free-standing package. Whether artifacts share across DISTINCT
# STANDALONE PACKAGES into one target directory is a different question
# from whether they share across members of one workspace, and probe 04
# does not answer it. This one uses the tool's shape.
#
# It also tests two things probe 04 does not: whether a second pass
# rebuilds (thrash), and whether the result depends on build ORDER.
#
# The `--config profile.release.*` flags are the ones the tool passes
# (`src/bench.rs:317-324`), because `--config` participates in the
# fingerprint and leaving it out would measure a different invocation.
#
# NEGATIVE CONTROLS, stated before the run:
#   (1) every build must SUCCEED. A standalone package refusing a foreign
#       --target-dir would void everything below.
#   (2) per-arm target dirs must cost strictly MORE than the shared dir,
#       or the probe distinguishes nothing.
#   (3) the shared dir must cost strictly FEWER than the arm count, or
#       nothing was shared and the proposal is empty.
set -uo pipefail
cd "$(dirname "$0")"

CFG=(--config 'profile.release.opt-level=3'
     --config 'profile.release.lto="fat"'
     --config 'profile.release.codegen-units=1')

# One `.fingerprint/support-*` directory per compilation of `support`.
# Counted this way rather than by artifact files because cargo hardlinks
# an uplifted copy beside the hashed one, which reads as two.
fp() { find "$@" -type d -name 'support-*' -path '*fingerprint*' 2>/dev/null | wc -l | tr -d ' '; }

build() { # build <arm> <target-dir>
  cargo build --release -q "${CFG[@]}" \
    --manifest-path "arm-$1/Cargo.toml" --target-dir "$2" 2>/dev/null
}

rm -rf shared alt ta tb tc arm-*/target support/target
fails=0

echo "cargo:  $(cargo --version)"
echo

echo "=== S1: per-arm target dir (what build_flat_variants does today) ==="
for a in a b c; do build $a "$PWD/t$a" || fails=$((fails+1)); done
S1=$(fp ta tb tc)
echo "  compilations of \`support\`: $S1"

echo
echo "=== S2: one shared target dir, order a,b,c ==="
for a in a b c; do build $a "$PWD/shared" || fails=$((fails+1)); done
S2=$(fp shared)
echo "  compilations of \`support\`: $S2"
echo "  cdylibs produced:          $(ls shared/release 2>/dev/null | grep -cE '\.(dylib|so)$' || true)"

echo
echo "=== S3: the SAME shared dir, a second pass. Does anything rebuild? ==="
for a in a b c; do build $a "$PWD/shared" || fails=$((fails+1)); done
S3=$(fp shared)
echo "  compilations of \`support\` after two passes: $S3  (was $S2)"

echo
echo "=== S4: a fresh shared dir, ALTERNATING order a,b,a,b,a,b ==="
for a in a b a b a b; do build $a "$PWD/alt" || fails=$((fails+1)); done
S4=$(fp alt)
echo "  compilations of \`support\`: $S4  (two arms, two feature sets)"

echo
echo "=== isolation: is each cdylib still a closed linkage unit under S2? ==="
for a in a b c; do
  lib=$(ls shared/release/libarm_$a.dylib shared/release/libarm_$a.so 2>/dev/null | head -1)
  n=$( (nm -g "$lib" 2>/dev/null || true) | { grep -c 'common' || true; } )
  echo "  libarm_$a: undefined/external symbols matching 'common' = $n"
done

echo
echo "=== NEGATIVE CONTROLS ==="
rc=0
if [ "$fails" -eq 0 ]; then echo "  (1) PASS: every --manifest-path build into a foreign target dir succeeded"
else echo "  (1) FAILED: $fails build(s) failed; everything above is void"; rc=1; fi
if [ "$S1" -gt "$S2" ]; then echo "  (2) PASS: per-arm S1=$S1 > shared S2=$S2"
else echo "  (2) FAILED: S1=$S1 not greater than S2=$S2; nothing distinguished"; rc=1; fi
if [ "$S2" -lt 3 ]; then echo "  (3) PASS: S2=$S2 < 3 arms, so matching feature sets shared"
else echo "  (3) FAILED: S2=$S2; standalone packages shared nothing"; rc=1; fi

echo
echo "=== FINDINGS ==="
if [ "$S3" -eq "$S2" ]; then echo "  no thrash: a repeat pass adds 0 compilations"
else echo "  THRASH: a repeat pass added $((S3-S2)) compilations"; fi
if [ "$S4" -eq 2 ]; then echo "  order-independent: alternating a,b x3 still costs 2"
else echo "  ORDER-DEPENDENT: alternating a,b x3 costs $S4, not 2"; fi
exit $rc
