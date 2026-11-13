#!/bin/bash
# 006/p1: reply to 001 q2. A research crate at <data>/sketch/Cargo.toml with path = "../../../crates/lib",
# data reached through the symlink mock/research -> .git/mock-data/research. Which invocations resolve?
set -u
S=${1:?scratch}; rm -rf $S/r1; mkdir -p $S/r1/m/crates/lib/src $S/r1/m/.git/mock-data/research/sketch/src && cd $S/r1/m
export CARGO_TARGET_DIR=$S/r1/target CARGO_HOME=${CARGO_HOME:-$HOME/.cargo} CARGO_NET_OFFLINE=true
printf '[package]\nname="lib"\nversion="0.1.0"\nedition="2021"\n' > crates/lib/Cargo.toml; echo 'pub fn f()->u8{1}' > crates/lib/src/lib.rs
printf '[package]\nname="sk"\nversion="0.1.0"\nedition="2021"\n[dependencies]\nlib={path="../../../crates/lib"}\n[workspace]\n' > .git/mock-data/research/sketch/Cargo.toml
echo 'fn main(){println!("{}",lib::f())}' > .git/mock-data/research/sketch/src/main.rs
mkdir -p mock; ln -s ../.git/mock-data/research mock/research
t() { local name=$1; shift; if out=$("$@" 2>&1); then echo "OK     $name"; else echo "FAILED $name: $(echo "$out" | grep -m1 -E 'error|Caused|unable|No such')"; fi; }
t "A cd into the link, cargo metadata (physical cwd)"      bash -c 'cd mock/research/sketch && cargo metadata --format-version 1 >/dev/null'
t "B from the root, --manifest-path through the link"      cargo metadata --format-version 1 --manifest-path mock/research/sketch/Cargo.toml
t "C shell cat ../../../crates/lib/Cargo.toml in the link" bash -c 'cd mock/research/sketch && cat ../../../crates/lib/Cargo.toml >/dev/null'
t "D cd -P not needed: PWD logical, cargo --manifest-path Cargo.toml" bash -c 'cd mock/research/sketch && cargo metadata --format-version 1 --manifest-path "$PWD/Cargo.toml" >/dev/null'
# control: the real directory (not a link) in the tree
rm mock/research; mkdir -p mock/research && cp -r .git/mock-data/research/. mock/research/
t "E control, real directory, cd + cargo metadata"         bash -c 'cd mock/research/sketch && cargo metadata --format-version 1 >/dev/null'
