#!/bin/bash
# Does the pre-command copy (core.fsmonitor, hook direct, no git-shook) hold the edit before the verbs that
# t2 (008_probes) showed destroying it silently: merge, plain checkout, pull of a commit that tracks a regular
# file at the hidden data path? Ad-hoc spike. All stdin from /dev/null.
source /home/user/.worktrees/seat001/mock/research/202611120700_data-ref-architecture/008_probes/common.sh
R=${1:?}; rm -rf "$R"; mkdir -p "$R"
for verb in "merge polluted" "checkout -q polluted" "reset -q --hard polluted"; do
  mk "$R/m"; git worktree add -q mock/rdata data; echo mock/rdata/ >> .git/info/exclude
  git branch polluted dev; git checkout -q polluted; mkdir -p mock/rdata.tmp; git checkout -q dev
  git checkout -q polluted; mv mock/rdata /tmp/rdata.$$ 2>/dev/null; mkdir -p mock/rdata; echo POLLUTION > mock/rdata/N.md; git add -f mock/rdata/N.md; git commit -qm pollute; git checkout -q dev; rm -rf mock/rdata; mv /tmp/rdata.$$ mock/rdata
  CACHE="$R/cache.$(echo $verb | tr ' ' _)"; mkdir -p "$CACHE"
  cat > "$R/hook.sh" <<H
#!/bin/sh
if [ -z "\${SNAP:-}" ]; then export SNAP=1; cp -a "$PWD/mock/rdata/." "$CACHE/\$(date +%s%N)/" 2>/dev/null || { d="$CACHE/\$(date +%s%N)"; mkdir -p "\$d"; cp -a "$PWD/mock/rdata/." "\$d/"; }; fi
printf 'tok\0/\0'
H
  chmod +x "$R/hook.sh"; git config core.fsmonitor "$R/hook.sh"; echo EDIT >> mock/rdata/N.md
  git $verb </dev/null >/dev/null 2>&1
  printf '%-26s on disk edit=%s data file=%s | cache copies holding the edit: %s\n' "$verb" "$(grep -c EDIT mock/rdata/N.md 2>/dev/null; true)" "$(tr '\n' '|' < mock/rdata/N.md 2>/dev/null)" "$(grep -l EDIT "$CACHE"/*/N.md 2>/dev/null | wc -l)"
done
