#!/usr/bin/env bash
# Probe 06: an arm cannot be asked which points it implements, so a manifest
# point the arm does not have is discovered by calling it and aborting.
# Set FW to a feat/bench-consolidation checkout, then: bash run.sh
set -uo pipefail
cd "$(dirname "$0")"
: "${FW:?set FW to the framework checkout}"
sed -i.bak "s|path = \"[^\"]*/bench-core\"|path = \"$FW/bench-core\"|; s|path = \"[^\"]*/bench-macro\"|path = \"$FW/bench-macro\"|" arm/Cargo.toml runner/Cargo.toml
cargo build --release -q --manifest-path arm/Cargo.toml
cargo build --release -q --manifest-path runner/Cargo.toml
LIB=$(ls arm/target/release/libarm.dylib arm/target/release/libarm.so 2>/dev/null | head -1)
echo "=== NEGATIVE CONTROL: the declared point n=64 ==="
./runner/target/release/runner "$PWD/$LIB" 64; echo "control exit=$?  (must be 0)"
echo
echo "=== an undeclared manifest point n=128 ==="
./runner/target/release/runner "$PWD/$LIB" 128; echo "exit=$?  (134 = SIGABRT)"
