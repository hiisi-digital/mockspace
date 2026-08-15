#!/usr/bin/env bash
# Probe: do the framework's `ignore`d doc examples compile?
#
# Every doctest in the four bench crates is fenced ```ignore, so rustdoc
# never builds one. Each bin transcribes one example verbatim between
# BEGIN/END markers, plus the minimum scaffolding the example's own prose
# names but does not show.
#
# THE CONTROL THIS PROBE NEEDED AND DID NOT HAVE ON ITS FIRST RUN: the
# path deps are RELATIVE, so the probe compiles against whichever branch
# the repository is checked out at. Run one, attribute to another, and
# every "FAILS" is an artifact of the checkout rather than a finding.
# The tree identity is therefore printed first and is part of the result.
#
# NEGATIVE CONTROLS:
#   C1 at least one bin must COMPILE. If every one fails, the probe is
#      measuring its own setup (a missing dep, a wrong edition) and not
#      the examples.
#   C2 ex2a (verbatim) and ex2b (the claim the example's comment makes)
#      must not agree. If they do, the probe cannot tell code from
#      comment and ex2 says nothing.
set -u
cd "$(dirname "$0")"
echo "tree:   $(git rev-parse --abbrev-ref HEAD) @ $(git rev-parse --short HEAD)"
echo "rustc:  $(rustc --version)"
echo
pass=0; fail=0
for b in ex1_byte_routine_module ex2a_dispatch_verbatim ex2b_dispatch_comment \
         ex3_routine_spec ex4_hooks_struct_update ex5_driver_module; do
  printf '%-28s ' "$b"
  if cargo build -q --bin "$b" 2>"/tmp/probe_$b.txt"; then
    echo "COMPILES"; pass=$((pass+1))
  else
    echo "FAILS: $(grep -m1 '^error' "/tmp/probe_$b.txt")"; fail=$((fail+1))
  fi
done
echo
echo "C1 at least one bin compiles: $([ $pass -gt 0 ] && echo PASS || echo 'FAIL - findings void')"
echo "compiled $pass, failed $fail"
