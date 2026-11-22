#!/bin/bash
# Spike k2: a replacement rule for the shipped `mockspace-generated` driver, which keeps ours whole for every
# generated file a regeneration follows (src/bootstrap/driver_script.rs, the `generated && followed_up` block).
# Rule under test: keep ours (and note a regeneration owed) ONLY when both ours and theirs are blobs the engine
# produced (the produced set of k1); otherwise run git merge-file and return its status, so a human edit is
# merged or left as a visible conflict, never dropped. Control: the shipped behaviour, keep ours whole.
set -u
exec </dev/null
S=${1:?scratch}; case "$S" in */p17/*) ;; *) echo "refusing scratch $S"; exit 2;; esac
rm -rf "$S"; mkdir -p "$S"; cd "$S"
export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@t GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@t HOME="$S/home"; mkdir -p "$HOME"
SET="$S/produced.set"; : > "$SET"
cat > "$S/rule.sh" <<H
#!/bin/sh
# %O %A %B
a=\$(git hash-object "\$2"); b=\$(git hash-object "\$3")
if grep -qx "\$a" "$SET" && grep -qx "\$b" "$SET"; then echo "\$a kept, regeneration owed" >> "$S/owed"; exit 0; fi
git merge-file -L ours -L base -L theirs "\$2" "\$1" "\$3"
H
printf '#!/bin/sh\nexit 0\n' > "$S/keepours.sh"; chmod +x "$S"/*.sh
gen() { printf '# Doc version %s\n\nIntro line.\n\nClosing paragraph.\n' "$1"; }
eng() { gen "$1" > G.md; git hash-object G.md >> "$SET"; }
for DRV in shipped rule; do for CASE in "both regenerated" "PR literal edit vs regeneration" "PR computed-line edit vs regeneration"; do
  rm -rf r; git init -q -b trunk r; cd r; echo 'G.md merge=gen' > .gitattributes
  [ $DRV = shipped ] && git config merge.gen.driver "$S/keepours.sh %O %A %B" || git config merge.gen.driver "$S/rule.sh %O %A %B"
  eng 1; git add -A; git commit -qm v1; git checkout -q -b side
  case "$CASE" in
    "both regenerated") eng 2;;
    "PR literal edit vs regeneration") sed -i 's/Intro line./Intro line, by a PR./' G.md;;
    "PR computed-line edit vs regeneration") sed -i 's/version 1/version 1 (beta)/' G.md;;
  esac; git commit -qam side; git checkout -q trunk; eng 3; git commit -qam v3
  git merge -q --no-edit side >/dev/null 2>&1; rc=$?
  printf '%-8s %-40s merge rc=%s G=%s\n' $DRV "$CASE" $rc "$(tr '\n' '|' < G.md | cut -c1-90)"
  cd ..
done; done
