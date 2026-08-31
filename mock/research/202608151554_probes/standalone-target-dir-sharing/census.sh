#!/usr/bin/env bash
# Census: how much sharing is structurally AVAILABLE across the real arm
# crates, which is what decides the size of the shared-target-dir win.
#
# This is a count of compilation units, not a timing. Nothing here is a
# benchmark and nothing here prices anything in seconds.
#
# The question the sibling's section 4 leaves open is "how much". A shared
# target dir shares an artifact only when the fingerprint matches, so the
# win is bounded by how many DISTINCT feature resolutions the real arms
# induce on the crates they share. That is a grep.
#
# NEGATIVE CONTROL: the framework dep lines must NOT all be identical
# across consumers. If they were, the "distinct pins" number would be 1 by
# construction and the census would be measuring nothing. vehje pins two
# different revs, which is what makes the count informative.
set -uo pipefail
W="${1:-$HOME/Dev/clause-dev}"
R="$W/arvo/mock/benches $W/vehje/mock/benches $W/hilavitkutin/mock/benches $W/kirjo/mock/benches"

echo "=== arm crates per consumer, and the composed form's uptake ==="
tot=0
for r in arvo vehje hilavitkutin kirjo; do
  n=$(find "$W/$r/mock/benches" -name Cargo.toml -path '*variants*' 2>/dev/null | wc -l | tr -d ' ')
  a=$(find "$W/$r/mock/benches" -type d -name arms 2>/dev/null | wc -l | tr -d ' ')
  tot=$((tot+n))
  printf "  %-14s variants/ arm crates = %-5s  <member>/arms/ dirs = %s\n" "$r" "$n" "$a"
done
echo "  total arm crates: $tot"
echo
echo "  Every one is on the legacy variants/ path, which build_flat_variants"
echo "  builds with cargo_build_at(.., None) at src/bench.rs:801-806, i.e."
echo "  with NO --target-dir. arm_target_dir (tree.rs:644) is reached only"
echo "  from src/bench.rs:590, the generated-arm path, which has no users."
echo
echo "=== do any arms vary the framework's own feature axis? ==="
for c in mockspace-bench-core mockspace-bench-macro; do
  echo "  $c, distinct dep lines with the source and pin normalised away:"
  find $R -name Cargo.toml -path '*variants*' -exec grep -h "^$c" {} + 2>/dev/null |
    sed -E 's/(rev|branch)[[:space:]]*=[[:space:]]*"[^"]*"/PIN/; s/(git|path)[[:space:]]*=[[:space:]]*"[^"]*"/SRC/; s/[[:space:]]+//g' |
    sort | uniq -c | sed 's/^/    /'
done
echo
echo "=== distinct PINS, which is what actually partitions the fingerprints ==="
find $R -name Cargo.toml -path '*variants*' -exec grep -h '^mockspace-bench-core' {} + 2>/dev/null |
  grep -oE '(rev|branch)[[:space:]]*=[[:space:]]*"[^"]*"' | sed 's/[[:space:]]//g' |
  sort | uniq -c | sed 's/^/  /'
echo
echo "=== distinct dependency+feature signatures per consumer ==="
echo "  (the structural bound on how much the consumer's OWN support crates"
echo "   can share; the framework crates are bounded by the pin count above)"
python3 - "$W" <<'PY'
import sys,glob,collections
W=sys.argv[1]
for repo in ("arvo","vehje","hilavitkutin","kirjo"):
    t=sorted(glob.glob(f"{W}/{repo}/mock/benches/**/variants/*/Cargo.toml",recursive=True))
    if not t: continue
    sigs=collections.Counter()
    for f in t:
        s=open(f).read()
        blk=s.split("[dependencies]",1)[1] if "[dependencies]" in s else ""
        blk=blk.split("[profile",1)[0].split("[workspace",1)[0]
        norm=tuple(sorted(l.split("=",1)[0].strip()+("F" if "features" in l else "")
                          for l in blk.splitlines()
                          if l.strip() and not l.strip().startswith('#')))
        sigs[norm]+=1
    print(f"    {repo:<14} {len(t):>5} arms -> {len(sigs)} distinct signatures")
PY
