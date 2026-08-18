#!/bin/sh
# P4. `mock bench test --release` runs a DEBUG test pass and reports success.
#
# Three subcommands (`run`, `report`, `test`) each build an `extra` list of
# `--`-prefixed arguments and forward it: cmd_run to the spawned driver,
# cmd_test to cargo. None of them can ever receive one. The dispatcher builds
# `bench_args` from `positional_args` (src/entry/dispatch.rs:416), and
# `positional_args` drops every flag at src/entry/dispatch.rs:193:
#
#     if arg.starts_with('-') { continue; }
#
# So the flag-forwarding code in all three is unreachable through the only
# entry point the tool has, and the flag is accepted, silently discarded, and
# the run reports a pass.
#
# NEGATIVE CONTROL, stated before the run, and it has to separate two layers.
#  (a) A POSITIONAL argument must be shown to reach bench::cmd, or "flags are
#      dropped" is indistinguishable from "nothing reaches bench::cmd". Proof:
#      `mock bench run <unknown>` must produce the "not found in bench.toml"
#      refusal, which only fires if the name arrived.
#  (b) A FLAG must be shown NOT to reach cargo. Proof: cargo's observed argv
#      under a shim must not contain `--release`.
# If (a) fails the instrument proves nothing. If (b) shows `--release`
# present, the finding is refuted.
set -e
ROOT=$(cd "$(dirname "$0")/../../../.." && pwd)
BIN="$ROOT/target/debug/mockspace"
[ -x "$BIN" ] || { echo "build the tool first"; exit 2; }
FX=$(mktemp -d); SHIMBIN=$(mktemp -d)
cat > "$SHIMBIN/cargo" <<'EOF'
#!/bin/sh
printf '%s\n' "$*" >> "$SHIM_LOG"
exit 1
EOF
chmod +x "$SHIMBIN/cargo"
mkdir -p "$FX/mock/benches/support/alpha/src" "$FX/mock/benches/variants/arm"
printf '[lints]\n' > "$FX/mock/mockspace.toml"
(cd "$FX" && git init -q . 2>/dev/null) || true
printf '[package]\nname = "alpha"\nversion = "0.0.0"\nedition = "2024"\n' > "$FX/mock/benches/support/alpha/Cargo.toml"
printf '#[cfg(test)]\nmod t { #[test] fn a() { assert_eq!(1 + 1, 2); } }\n' > "$FX/mock/benches/support/alpha/src/lib.rs"
printf '[package]\nname = "b"\nversion = "0.0.0"\nedition = "2024"\n' > "$FX/mock/benches/Cargo.toml"
printf '[bench.demo]\ntitle = "d"\nworkload = "default"\nvariants = ["arm"]\n' > "$FX/mock/benches/bench.toml"
printf '[package]\nname = "arm"\nversion = "0.0.0"\nedition = "2024"\n' > "$FX/mock/benches/variants/arm/Cargo.toml"

echo "=== control (a): does a POSITIONAL argument reach bench::cmd? ==="
CA=$( (cd "$FX/mock" && "$BIN" bench run no-such-bench-name) 2>&1 | grep -c "not found in bench.toml" || true)
echo "  'not found in bench.toml' refusal seen: $CA (want 1)"

echo "=== control (b): does a FLAG reach cargo? ==="
SHIM_LOG="$FX/argv.log"; : > "$SHIM_LOG"
(cd "$FX/mock" && PATH="$SHIMBIN:$PATH" SHIM_LOG="$SHIM_LOG" "$BIN" bench test --release) >/dev/null 2>&1 || true
echo "  cargo argv observed: $(cat "$SHIM_LOG")"
HASREL=$(grep -c -- '--release' "$SHIM_LOG" || true)
echo "  '--release' present in cargo argv: $HASREL (want 0)"

echo
if [ "$CA" -ge 1 ] && [ "$HASREL" -eq 0 ]; then
  echo "CONTROL: ok. Positionals arrive; flags do not."
  echo "VERDICT: mock bench test --release runs cargo test with no --release."
  echo "         The flag is dropped at src/entry/dispatch.rs:193 before"
  echo "         bench::cmd is called, so the extra-forwarding code in"
  echo "         cmd_test, cmd_run and cmd_report is unreachable via the CLI."
  echo "         The command accepts the flag, ignores it, and reports a pass."
else
  echo "CONTROL FAILED or refuted: CA=$CA HASREL=$HASREL -- result suppressed"; exit 1
fi
rm -rf "$FX" "$SHIMBIN"
