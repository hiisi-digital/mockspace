#!/bin/bash
# (a) the directory named @ in a git worktree: tracked fine (011 n1), but @ is also git's alias for HEAD.
#     Which everyday commands become ambiguous without "--"?
# (b) ~mock as a directory name: what shells do with a leading tilde.
# (c) the instance root at depth 1 (.mock, git~1): ../ research paths resolve at the old depth (cargo).
R=${1:?}; rm -rf "$R"; mkdir -p "$R"; cd "$R"
export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@t GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@t
git init -q -b d r; cd r; mkdir -p '@/crates/foo' crates/lib/src; echo T > '@/crates/foo/README.md.tmpl'
printf '[package]\nname="lib"\nversion="0.1.0"\nedition="2021"\n' > crates/lib/Cargo.toml; echo 'pub fn f(){}' > crates/lib/src/lib.rs
git add -A; git commit -qm x; echo more >> '@/crates/foo/README.md.tmpl'
echo "== (a) commands with a path named @ and no --"
for c in "git diff @" "git log --oneline @" "git show @:crates/lib/Cargo.toml" "git add @" "git checkout @" "git rm -r --cached @" "git status --short @" "git ls-files @" "git diff -- @" "git add -- @"; do out=$($c 2>&1 | head -1 | cut -c1-90); printf '%-34s -> %s\n' "$c" "${out:-ok (no output)}"; done
git reset -q --hard
echo "== (b) ~mock"; mkdir '~mock'; echo x > '~mock/f'; echo "unquoted echo ~mock/f -> $(echo ~mock/f)   quoted -> $(echo '~mock/f')   ls ~mock -> $(ls ~mock 2>&1 | head -1 | cut -c1-60)   cat via quoted: $(cat '~mock/f')"
echo "(a user named mock would make unquoted ~mock expand to that home: $(getent passwd mock | cut -d: -f6; true))"
echo "== (c) cargo at depth 1 roots: '.mock/research' and 'git~1/research' with ../../../crates/lib"
for root in .mock 'git~1'; do mkdir -p "$root/research/sketch/src"; printf '[package]\nname="sk"\nversion="0.1.0"\nedition="2021"\n[dependencies]\nlib={path="../../../crates/lib"}\n' > "$root/research/sketch/Cargo.toml"; echo 'fn main(){ lib::f(); }' > "$root/research/sketch/src/main.rs"
  (cd "$root/research/sketch" && cargo metadata --offline --format-version 1 >/dev/null 2>&1 && echo "$root/research/sketch: cargo OK" || echo "$root/research/sketch: cargo FAILED"); done
