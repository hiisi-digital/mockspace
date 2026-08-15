#!/usr/bin/env bash
# Probe 04: per-arm target directories, feature contamination, and the one
# parameter that fixes the rebuild cost without breaking isolation.
#
# Three cdylib arms over one shared support crate, under the harness's own
# release profile (fat LTO, one codegen unit). `arm-a` asks for the support
# crate's `fast` feature; `arm-b` and `arm-c` do not.
#
# Three build shapes:
#   A  one cargo invocation over a workspace containing all three
#   B  one invocation per arm, ONE shared --target-dir
#   C  one invocation per arm, one --target-dir EACH   (what the tool does)
#
# What is counted is COMPILATIONS of `shared`, taken from `.fingerprint/`
# directories, one per compilation. A naive count of files named libshared*
# reads 2 for a single compile because cargo hardlinks an uplifted copy
# beside the hashed one; that defect was in this probe's first version and is
# why the count is not taken that way.
#
# NEGATIVE CONTROLS, all three required:
#   (1) C must cost strictly more compilations than B, or the probe
#       distinguishes nothing.
#   (2) B must cost strictly fewer than the arm count, or nothing is shared.
#   (3) A must show ONE compilation, which IS the contamination: two arms
#       with different declared features cannot both be correct against one
#       rlib. If A shows more than one, the workspace shape is safe after all
#       and this probe's central claim is void.
set -uo pipefail
cd "$(dirname "$0")"

fp() { find "$@" -type d -name 'shared-*' -path '*fingerprint*' 2>/dev/null | wc -l | tr -d ' '; }
dylibs() { ls "$1" 2>/dev/null | { grep -cE '\.(dylib|so)$' || true; }; }

rm -rf target shared_td ta tb tc

echo "=== A: one workspace, one invocation ==="
cargo build --release -q 2>/dev/null
A=$(fp target)
echo "  compilations of \`shared\`: $A"

echo
echo "=== B: one invocation per arm, ONE shared --target-dir (proposed) ==="
for a in a b c; do cargo build --release -q -p arm-$a --target-dir "$PWD/shared_td" 2>/dev/null; done
B=$(fp shared_td)
echo "  compilations of \`shared\`: $B"
echo "  cdylibs produced:          $(dylibs shared_td/release)"

echo
echo "=== C: one invocation per arm, one --target-dir each (today) ==="
for a in a b c; do cargo build --release -q -p arm-$a --target-dir "$PWD/t$a" 2>/dev/null; done
C=$(fp ta tb tc)
echo "  compilations of \`shared\`: $C"

echo
echo "=== isolation under B: is each cdylib still a closed linkage unit? ==="
for a in a b c; do
  lib=$(ls shared_td/release/libarm_$a.dylib shared_td/release/libarm_$a.so 2>/dev/null | head -1)
  n=$( (nm -g "$lib" 2>/dev/null || true) | { grep -c 'common' || true; } )
  echo "  libarm_$a: symbols matching 'common' crossing the boundary = $n"
done

echo
echo "=== NEGATIVE CONTROLS ==="
rc=0
if [ "$C" -gt "$B" ]; then echo "  (1) PASS: isolated C=$C > shared B=$B"
else echo "  (1) FAILED: C=$C not greater than B=$B; probe distinguishes nothing"; rc=1; fi
if [ "$B" -lt 3 ]; then echo "  (2) PASS: B=$B < 3 arms, so identical feature sets shared an artifact"
else echo "  (2) FAILED: B=$B shared nothing across three arms"; rc=1; fi
if [ "$A" -eq 1 ]; then echo "  (3) PASS: A=$A, so the workspace shape handed arm-b and arm-c a \`shared\` built with arm-a's feature"
else echo "  (3) FAILED: A=$A, the workspace shape did not unify; the contamination claim is void"; rc=1; fi
exit $rc
