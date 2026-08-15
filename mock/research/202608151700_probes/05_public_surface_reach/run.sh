#!/usr/bin/env bash
# Probe 05: what of the bench framework's public surface does anything call?
#
# For every module-level `pub fn` / `pub struct` / `pub enum` in the four bench
# crates, classify references OUTSIDE the declaring file into:
#   CALLED         at least one reference that is not a `use`/`pub use` line
#   REEXPORT-ONLY  referenced only by `use` lines (i.e. exported and never used)
#   UNREFERENCED   no reference anywhere, in the framework or in the consumers
#
# Universes searched: the framework's own crates and binary, and the four
# consumer bench trees, which are the callers a library exists for.
#
# LIMITATION, stated because it changes how the output reads: a type reachable
# only as a FIELD of an exported struct (a serde config section) is referenced
# only inside its declaring file and lands in UNREFERENCED. That is the
# expected answer for such a type, not a defect. Only items whose whole point
# is to be called carry a finding here.
#
# NEGATIVE CONTROLS, both required:
#   (1) `drive`, which every consumer's driver calls, must classify CALLED
#       with a nonzero consumer count. A zero means the consumer paths are
#       wrong and every classification is an artifact.
#   (2) a fabricated name must classify UNREFERENCED. Otherwise the matcher
#       matches substrings and every count is inflated.
set -uo pipefail
FW="${FW:?set FW to the framework checkout}"
CONS="${CONS:-$HOME/Dev/clause-dev}"
CONSUMERS="$CONS/arvo/mock/benches $CONS/hilavitkutin/mock/benches $CONS/vehje/mock/benches $CONS/kirjo/mock/benches"
FWROOTS="$FW/bench-core/src $FW/bench-harness/src $FW/bench-macro/src $FW/bench-matrix/src $FW/src $FW/benches"

hits() { grep -rwF --include='*.rs' "$1" $3 2>/dev/null | grep -v "^$2:"; }

classify() { # name declfile -> "CLASS fwuse fwcall couse cocall"
  local n="$1" d="$2"
  local fw co
  fw=$(hits "$n" "$d" "$FWROOTS")
  co=$(hits "$n" "$d" "$CONSUMERS")
  local fwc coc fwu cou
  fwc=$(printf '%s\n' "$fw" | grep -v '^\s*$' | grep -vcE ':\s*(pub )?use |:\s+[A-Za-z_0-9]+,\s*$' || true)
  coc=$(printf '%s\n' "$co" | grep -v '^\s*$' | grep -vcE ':\s*(pub )?use |:\s+[A-Za-z_0-9]+,\s*$' || true)
  fwu=$(printf '%s\n' "$fw" | grep -v '^\s*$' | grep -c . || true)
  cou=$(printf '%s\n' "$co" | grep -v '^\s*$' | grep -c . || true)
  local cls=UNREFERENCED
  [ "$((fwu+cou))" -gt 0 ] && cls=REEXPORT-ONLY
  [ "$((fwc+coc))" -gt 0 ] && cls=CALLED
  echo "$cls $fwu $fwc $cou $coc"
}

echo "=== NEGATIVE CONTROLS ==="
r1=$(classify drive "$FW/bench-harness/src/driver/mod.rs"); echo "  (1) drive: $r1  (must be CALLED with nonzero consumer calls)"
r2=$(classify zzz_no_such_item_anywhere /dev/null);        echo "  (2) fabricated name: $r2  (must be UNREFERENCED)"
case "$r1" in CALLED*) ;; *) echo "  CONTROL FAILED (1). Output void."; exit 1;; esac
[ "$(echo "$r1" | awk '{print $5}')" -gt 0 ] || { echo "  CONTROL FAILED (1): no consumer calls. Output void."; exit 1; }
case "$r2" in UNREFERENCED*) ;; *) echo "  CONTROL FAILED (2). Output void."; exit 1;; esac
echo "  both pass"
echo
printf '%-30s %-34s %-14s %s\n' "item" "declared in" "class" "fw_refs/fw_calls  cons_refs/cons_calls"
for f in "$FW"/bench-core/src/*.rs "$FW"/bench-harness/src/*.rs "$FW"/bench-harness/src/*/*.rs \
         "$FW"/bench-macro/src/*.rs "$FW"/bench-matrix/src/*.rs; do
  [ -f "$f" ] || continue
  case "$f" in *tests.rs) continue;; esac
  rel="${f#$FW/}"
  grep -oE '^pub (fn [a-z_0-9]+|struct [A-Za-z_0-9]+|enum [A-Za-z_0-9]+)' "$f" 2>/dev/null \
  | sed -E 's/^pub (fn|struct|enum) //' | sort -u | while read -r n; do
      [ -n "$n" ] || continue
      set -- $(classify "$n" "$f")
      [ "$1" = CALLED ] && continue
      printf '%-30s %-34s %-14s %s/%s  %s/%s\n' "$n" "$rel" "$1" "$2" "$3" "$4" "$5"
  done
done
