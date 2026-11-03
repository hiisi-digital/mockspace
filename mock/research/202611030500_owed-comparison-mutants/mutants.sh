#!/usr/bin/env bash
# Mutation check for what `deprecate` and `unlock` say the replacement owes the
# `deprecation-comparison` lint (`owed_comparison` in lint-rules, `owed_note` in
# the launcher). Each line of the table below is one mutant: the crate whose
# tests judge it, the file, a perl substitution that must match exactly once, and
# a name. A mutant is killed when `cargo test` fails with the change applied. A
# build failure is reported apart, since it kills nothing. Run from the repo root.
set -u
killed=0; survived=0; broken=0
while IFS='|' read -r name crate file expr; do
  [ -z "$name" ] && continue
  case "$crate" in
    lint-rules) f="lint-rules/src/$file"; cmd=(cargo test -p mockspace-lint-rules --lib owed_comparison) ;;
    launcher)   f="src/$file";            cmd=(cargo test -p mockspace --lib design_round::tests::owed) ;;
    *) echo "BAD-CRATE $name"; broken=$((broken+1)); continue ;;
  esac
  cp "$f" "$f.orig"
  perl -0pi -e "s#${expr%%@@*}#${expr#*@@}#" "$f"
  if cmp -s "$f" "$f.orig"; then
    echo "NO-MATCH  $name"; broken=$((broken+1))
  else
    out=$("${cmd[@]}" 2>&1)
    if echo "$out" | grep -qE '^error(\[E|: could not compile)'; then
      echo "BUILD     $name"; broken=$((broken+1))
    elif echo "$out" | grep -q 'test result: ok'; then
      echo "SURVIVED  $name"; survived=$((survived+1))
    else
      echo "killed    $name"; killed=$((killed+1))
    fi
  fi
  cp "$f.orig" "$f"; rm "$f.orig"; touch "$f"
done <<'TABLE'
a stub owes a comparison|lint-rules|owed_comparison.rs|if !owes_a_comparison\(deprecated_text, ClStatus::Active\) \{@@if false {
the section header changed|lint-rules|owed_comparison.rs|"## Comparison to deprecated changelist\\n"@@"## Comparison\\n"
no subheader for a header|lint-rules|owed_comparison.rs|for header in extract_h2_headers\(deprecated_text\) \{@@for header in Vec::<String>::new() {
subheaders copied as written|lint-rules|owed_comparison.rs|section\.push_str\(&header\.to_lowercase\(\)\);@@section.push_str(&header);
a name the section holds not reported|lint-rules|owed_comparison.rs|\.filter\(\|n\| section\.contains\(n\.as_str\(\)\)\)@@.filter(|_| false)
every name reported as met|lint-rules|owed_comparison.rs|\.filter\(\|n\| section\.contains\(n\.as_str\(\)\)\)@@.filter(|_| true)
the names line left out of the note|launcher|design_round/owed.rs|if !owed\.names\.is_empty\(\) \{@@if false {
the met line left out of the note|launcher|design_round/owed.rs|if !owed\.met_by_the_section\.is_empty\(\) \{@@if false {
a stub says something|launcher|design_round/owed.rs|let owed = lint_rules::owed_comparison\(deprecated_text\)\?;@@let owed = lint_rules::owed_comparison(deprecated_text).unwrap_or(lint_rules::OwedComparison { section: String::new(), names: vec![], met_by_the_section: vec![] });
TABLE
echo "killed=$killed survived=$survived broken=$broken"
