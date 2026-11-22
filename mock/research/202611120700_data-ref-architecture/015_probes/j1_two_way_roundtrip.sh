#!/bin/bash
# Two-way shadow tree as git commands. A toy renderer: {{name}} becomes a value, {{table}} becomes the rows of a
# data file. G0 = render(T0) is what the git tree commits. A PR edits G. Which edits can `git merge-file` carry
# back into the template T, with base = G at the PR's merge-base, other = G at the PR tip, current = T? Then
# the round-trip check: render(T') must equal G1, or nothing is absorbed.
source "$(dirname "$0")/guard.sh"; scratch j1
cat > render.sh <<'H'
#!/bin/bash
# render <template> <data dir>
awk -v dd="$2" 'BEGIN{while((getline l < (dd "/kv"))>0){split(l,a,"="); kv[a[1]]=a[2]} }
{ if ($0=="{{table}}") { while((getline r < (dd "/rows"))>0) print "| " r " |"; close(dd "/rows") }
  else { line=$0; for (k in kv) gsub("{{" k "}}", kv[k], line); print line } }' "$1"
H
chmod +x render.sh; mkdir data; printf 'version=1.2\nname=foo\n' > data/kv; printf 'alpha\nbeta\ngamma\n' > data/rows
cat > T0 <<'H'
# {{name}}

Intro paragraph, first line.
Intro paragraph, second line.

version: {{version}}

{{table}}

Closing paragraph.
H
./render.sh T0 data > G0
echo "G0:"; sed 's/^/   /' G0
absorb() { # label, G1 file, optional current template
  local label=$1 G1=$2 T=${3:-T0}; cp "$T" Tp
  if git merge-file -p Tp G0 "$G1" > Tn 2>/dev/null; then
    ./render.sh Tn data > Rn
    # expected result: the PR's change applied to what the CURRENT template renders (equals G1 when T is unchanged)
    ./render.sh "$T" data > Gcur; cp Gcur Exp; git merge-file -p Exp G0 "$G1" > Exp2 2>/dev/null
    if cmp -s Rn Exp2; then v="CLEAN, and render(T') == render(T) + PR change: absorbed"; else v="clean merge but render(T') differs from the expected result: refuse"; fi
  else cp "$T" Tp; n=$(git merge-file -p Tp G0 "$G1" 2>/dev/null | grep -c '^<<<<<<<'); v="CONFLICT ($n hunk): not automatic"; fi
  printf '%-52s %s\n' "$label" "$v"
}
ed() { sed "$1" G0 > G1; }
ed 's/Intro paragraph, second line\./Intro paragraph, second line, edited by a PR./'; absorb "1 edit a literal prose line" G1
ed 's/^version: 1.2/version: 1.3/';               absorb "2 edit a line a placeholder produced" G1
ed 's/^| beta |/| beta2 |/';                       absorb "3 edit a row of the generated table" G1
ed '/^| gamma |/d';                                absorb "4 delete a generated table row" G1
ed 's/^Closing paragraph\./Closing paragraph.\n\nA paragraph added by a PR./'; absorb "5 insert a paragraph after literal text" G1
ed '/^Intro paragraph, first line\./d';            absorb "6 delete a literal line" G1
ed 's/^Closing paragraph\./Closing paragraph, edited./'; absorb "7 edit the literal line right after the table" G1
ed 's/^# foo/# bar/';                              absorb "8 edit the title a placeholder produced" G1
cp T0 T1; sed -i 's/^Closing paragraph\./Closing paragraph, upstream./' T1
ed 's/Intro paragraph, second line\./Intro paragraph, second line, edited by a PR./'; absorb "9 PR edit + template changed on another line" G1 T1
cp T0 T2; sed -i 's/Intro paragraph, second line\./Intro paragraph, second, upstream./' T2
absorb "10 PR edit + template changed on the SAME line" G1 T2
: > G1; absorb "11 PR empties the generated file" G1
echo "== base choice: G at the PR's merge-base against the latest G after a template change"
./render.sh T1 data > Gnew; ed 's/Intro paragraph, second line\./Intro paragraph, second line, edited by a PR./'
cp T1 Tp; git merge-file -p Tp G0 G1 > Tn 2>/dev/null; echo "base = G at merge-base: closing line in T' = [$(grep '^Closing' Tn)] (upstream change kept)"
cp T1 Tp; git merge-file -p Tp Gnew G1 > Tn 2>/dev/null; echo "base = latest G:           closing line in T' = [$(grep '^Closing' Tn)] (the PR is read as reverting the upstream change)"
echo "== regeneration without trivial conflicts: two branches regenerate G from different data, then merge"
git init -q -b dev rg; cd rg; cp ../G0 G; g add G; g commit -qm g0; g branch a; g branch b
g checkout -q a; sed -i 's/^version: 1.2/version: 1.3/' G; g commit -qam "regen a"
g checkout -q b; sed -i 's/^version: 1.2/version: 1.4/' G; g commit -qam "regen b"
g checkout -q a; g merge b -m m >/dev/null 2>&1; echo "plain merge of two regenerations: [$(g status --short | head -1)] conflict markers: $(grep -c '^<<<<<<<' G)"; g merge --abort
echo 'G merge=gen' > .gitattributes; g config merge.gen.driver 'true'
g merge b -m m >/dev/null 2>&1; echo "with merge=gen (driver keeps ours): rc=$? conflict markers: $(grep -c '^<<<<<<<' G); G says: $(grep '^version' G)"
printf 'version=1.4\nname=foo\n' > ../data/kv; ../render.sh ../T0 ../data > G; g commit -qam "regenerated from the newer data"
echo "after the post-merge regeneration from the newer data: $(grep '^version' G)"
