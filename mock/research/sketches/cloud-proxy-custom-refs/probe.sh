#!/usr/bin/env nutshell
# probe.sh - what a cloud container's git proxy does with custom refs
#
#   probe.sh <clone of a repository the session may push to> <a commit the remote already holds>
#
# Pushes the commit to a custom ref and to a branch, deletes the branch again, and fetches a ref
# outside refs/heads by its exact name, printing one line per step with git's exit status. The
# branch is made and deleted under `mock/probe-<pid>`; where the proxy refuses the delete, the line
# says so and the branch is left for a forge route to take away. Nothing else is written.
#
# Copyright (c) 2026 orgrinrt <ort@hiisi.digital>
# SPDX-License-Identifier: MPL-2.0

[ $# -eq 2 ] || { echo "usage: probe.sh <clone> <commit>" >&2; exit 2; }
clone=$1
commit=$2
name="probe-$$"

step() {
    local what=$1; shift
    if git -C "$clone" "$@" >/dev/null 2>&1; then echo "$what: accepted"; else echo "$what: refused ($?)"; fi
}

step "push to refs/mock/$name" push origin "$commit:refs/mock/$name"
step "push to refs/heads/mock/$name" push origin "$commit:refs/heads/mock/$name"
step "delete refs/heads/mock/$name" push origin --delete "refs/heads/mock/$name"
step "fetch refs/pull/1/head by name" fetch -q origin "refs/pull/1/head:refs/probe/$name"
git -C "$clone" update-ref -d "refs/probe/$name" 2>/dev/null
