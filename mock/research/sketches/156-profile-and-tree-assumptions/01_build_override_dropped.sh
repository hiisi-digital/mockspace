#!/bin/sh
# P1. Does a `[build]` override in bench.toml reach the builds on BOTH driver
# paths, or only on the generated one?
#
# `BuildSection`'s own doc (bench-harness/src/config.rs:147-150) promises:
#   "The tool passes the effective values on the command line (`--config`),
#    where a manifest cannot silently drop them"
#
# HISTORY. As first written this script asserted the DEFECT: fixture A came
# back with opt-level=3 while fixture B came back with 0. That run is kept
# beside it as 01_build_override_dropped_BEFORE_FIX.out. Since the fix
# (build_argv now takes the effective profile, and consumer_tree_profile reads
# the tree's own bench.toml) the script asserts the repaired behaviour, so it
# is a regression check rather than a demonstration.
#
# NEGATIVE CONTROL, stated before the run. Two fixtures declare an IDENTICAL
# `[build] opt-level = 0`. Fixture A has a consumer-owned driver
# (mock/benches/Cargo.toml exists, which is arvo's shape); fixture B has none,
# so the driver is generated. The two MUST disagree. If both show 0 the
# override is honoured everywhere and this probe refutes the finding. If both
# show 3 the override is dead everywhere and the mechanism is not the path
# split. Only A=3, B=0 supports the claim.
#
# cargo is shimmed to log its argv and exit 1: the argv is the whole
# measurement and nothing past the build is needed.
set -e
ROOT=$(cd "$(dirname "$0")/../../../.." && pwd)
BIN="$ROOT/target/debug/mockspace"
[ -x "$BIN" ] || { echo "build the tool first: cargo build --bin mockspace"; exit 2; }
FX=$(mktemp -d); SHIMBIN=$(mktemp -d)
cat > "$SHIMBIN/cargo" <<'EOF'
#!/bin/sh
printf '%s\n' "$*" >> "$SHIM_LOG"
exit 1
EOF
chmod +x "$SHIMBIN/cargo"

BUILD_SECTION='[build]
opt-level = 0
lto = "off"
codegen-units = 16
'
# ── fixture A: consumer-owned driver ──
mkdir -p "$FX/A/mock/benches/variants/arm"
printf '[lints]\n' > "$FX/A/mock/mockspace.toml"
printf '[package]\nname = "a-benches"\nversion = "0.0.0"\nedition = "2024"\n' > "$FX/A/mock/benches/Cargo.toml"
printf '%s\n[bench.demo]\ntitle = "demo"\nworkload = "default"\n' "$BUILD_SECTION" > "$FX/A/mock/benches/bench.toml"
printf '[package]\nname = "arm"\nversion = "0.0.0"\nedition = "2024"\n' > "$FX/A/mock/benches/variants/arm/Cargo.toml"

# ── fixture B: generated driver (the control) ──
mkdir -p "$FX/B/mock/benches/hash/arms/plusone/src"
printf '[lints]\n' > "$FX/B/mock/mockspace.toml"
printf '%s\n[timing]\npasses = 1\nruns_per_pass = 20\nbatch_size = 5\nharness_runs = 1\ncooldowns_ms = [0]\n' "$BUILD_SECTION" > "$FX/B/mock/benches/bench.toml"
printf 'title = "Plus one"\nworkload = "default"\narms = ["plusone"]\npoints = [64]\nmaster_seed = 7\n' > "$FX/B/mock/benches/hash/bench.toml"
echo 'pub fn x() {}' > "$FX/B/mock/benches/hash/arms/plusone/src/lib.rs"

for d in A B; do
  (cd "$FX/$d" && git init -q . 2>/dev/null) || true
  SHIM_LOG="$FX/$d.log"; : > "$SHIM_LOG"
  shape=$([ -f "$FX/$d/mock/benches/Cargo.toml" ] && echo "consumer-owned driver" || echo "generated driver")
  echo "=== fixture $d: $shape, bench.toml declares opt-level = 0 ==="
  (cd "$FX/$d/mock" && PATH="$SHIMBIN:$PATH" SHIM_LOG="$SHIM_LOG" "$BIN" bench run 2>&1 | tail -2)
  got=$(grep -o 'profile.release.opt-level=[0-9]*' "$SHIM_LOG" | sort -u)
  echo "  cargo was invoked with: ${got:-<no build reached>}"
  eval "GOT_$d=\$got"
done
echo
echo "declared in both fixtures : profile.release.opt-level=0"
echo "fixture A (consumer-owned): $GOT_A"
echo "fixture B (generated)     : $GOT_B"
echo
if [ "$GOT_A" = "profile.release.opt-level=0" ] && [ "$GOT_B" = "profile.release.opt-level=0" ]; then
  echo "CONTROL: ok, the declared 0 is distinguishable from the framework default 3."
  echo "VERDICT: both driver paths honour [build]. Before the fix, fixture A"
  echo "         built at 3 while declaring 0, with no error and no warning:"
  echo "         build_argv() took no config and passed a constant, and"
  echo "         profile_args_for() was called only from run_generated."
else
  echo "REGRESSION: a [build] override is being dropped. A=$GOT_A B=$GOT_B"
  echo "            (both must be profile.release.opt-level=0)"
  exit 1
fi
rm -rf "$FX" "$SHIMBIN"
