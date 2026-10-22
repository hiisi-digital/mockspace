#!/usr/bin/env bash
# Not in the table, because equivalent: dropping the lib.rs, main.rs and src/bin/<name>/main.rs
# roots changes nothing, since those files own their directory by name and the
# orphan walk reaches them first. Only src/bin/<name>.rs needs the root rule.
# Mutation check for the `single-source` lint. Each line of the table below is one
# mutant: the file, a perl substitution that must match exactly once, and a name.
# A mutant is killed when `cargo test` fails with the change applied. A build
# failure is reported apart, since it kills nothing. Run from the repo root.
set -u
SRC=lint-rules/src
killed=0; survived=0; broken=0
while IFS='|' read -r name file expr; do
  [ -z "$name" ] && continue
  f="$SRC/$file"
  cp "$f" "$f.orig"
  perl -0pi -e "s#${expr%%@@*}#${expr#*@@}#" "$f"
  if cmp -s "$f" "$f.orig"; then
    echo "NO-MATCH  $name"; broken=$((broken+1))
  else
    out=$(cargo test -p mockspace-lint-rules --lib single_source 2>&1)
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
suffix dropped everywhere|single_source/value.rs|if whole \{ number \} else \{ format!\("\{number\}\{suffix\}"\) \}@@number
visibility filter removed|single_source.rs|\.filter\(\|def\| !only_pub \|\| def\.fully_pub\(\)\)@@.filter(|_| true)
switch flipped|single_source.rs|COMPARE_ONLY_PUB: bool = true@@COMPARE_ONLY_PUB: bool = false
qualifier always pub|single_source.rs|if def\.vis\.is_empty\(\) \{ String::new\(\) \} else \{ format!\("\{\} ", def\.vis\) \}@@"pub ".to_string()
pub(crate) counts as pub|single_source/defs.rs|self\.vis == "pub"@@self.vis.starts_with("pub")
inline mod dir ignores the stem|single_source/defs.rs|file_dir: scope\.mod_dir\.join\(name\),\n( +)mod_dir:  scope\.mod_dir\.join\(name\),@@file_dir: scope.file_dir.join(name),\n\1mod_dir:  scope.file_dir.join(name),
bin file root removed|single_source/defs.rs|file\.ends_with\("\.rs"\) => Some\(true\)@@false => Some(true)
non-mod-rs file does not nest under its stem|single_source/defs.rs|if owns_dir \{ parent\.clone\(\) \} else \{ parent\.join\(stem\) \}@@parent.clone()
orphans not walked|single_source/defs.rs|if !walker\.reached\.contains\(&i\) \{@@if false {
TABLE
echo "killed=$killed survived=$survived other=$broken"
