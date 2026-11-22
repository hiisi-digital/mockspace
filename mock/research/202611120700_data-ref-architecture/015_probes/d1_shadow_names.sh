#!/bin/bash
# Candidate names for a directory at the root of a mock tree (or for a namespace), against git's revision and
# pathspec parsing. Worst case: a branch of the same name exists wherever git allows one. No "--" anywhere.
source "$(dirname "$0")/guard.sh"; scratch d1
cat > classify.sh <<'H'
#!/bin/bash
# one word for what git did with a bare argument
out=$(timeout 30 "$@" </dev/null 2>&1)
case "$out" in
  *ambiguous*) echo "AMBIG";;
  *"switching to"*|*"HEAD is now"*|*"detached"*|*"Switched to"*) echo "AS-REV";;
  *"unknown revision"*|*"bad revision"*|*"bad object"*) echo "no-path";;
  *fatal*|*error*) echo "err";;
  *) echo "ok";;
esac
H
chmod +x classify.sh
names=('@' shadow .shadow _shadow '#shadow' '%shadow' '=shadow' '+shadow' '-shadow' '$shadow' HEAD ORIG_HEAD shadow.lock)
printf '%-12s %-7s %-7s %-7s %-9s %-9s %-7s %-8s %-7s %s\n' name log diff show checkout rev-parse add restore status "branch-possible"
for n in "${names[@]}"; do
  [ -d repo ] && rm -r -- repo
  g init -q -b dev repo; cd repo; echo 1 > keep; g add keep; g commit -qm k
  mkdir -p -- "./$n" 2>/dev/null; echo a > "./$n/f"; g add -f -- "./$n/f" 2>/dev/null; g commit -qm "add $n" 2>/dev/null
  br=no; g branch -- "$n" >/dev/null 2>&1 && br=yes
  echo b >> "./$n/f"
  c() { "$R/classify.sh" git "$@"; }
  printf '%-12s %-7s %-7s %-7s %-9s %-9s %-7s %-8s %-7s %s\n' "$n" "$(c log --oneline "$n")" "$(c diff "$n")" "$(c show "$n")" "$(c checkout "$n")" "$(c rev-parse "$n")" "$(c add "$n")" "$(c restore "$n")" "$(c status -s "$n")" "$br"
  cd "$R"
done
echo "== bash, unquoted, name at the start of a word (what the shell does before git sees it)"
for n in '#shadow' '%shadow' '=shadow' '+shadow' '$shadow' '-shadow' '.shadow' '_shadow'; do printf '%-9s -> [%s]\n' "$n" "$(bash -c "echo $n/f" 2>&1 | head -1 | cut -c1-50)"; done
if command -v zsh >/dev/null 2>&1; then echo "== zsh"; for n in '=shadow' '%shadow' '#shadow'; do printf '%-9s -> [%s]\n' "$n" "$(zsh -c "echo $n/f" 2>&1 | head -1 | cut -c1-60)"; done
else echo "(zsh is not installed here; its rule that a word starting with = expands to a command path, and fails when none, is an impression)"; fi
echo "== can the name ever be a ref component? (git check-ref-format)"
for n in "${names[@]}"; do if git check-ref-format "refs/heads/$n" 2>/dev/null; then v=valid; else v=INVALID; fi; printf '%-12s %s\n' "$n" "$v"; done
