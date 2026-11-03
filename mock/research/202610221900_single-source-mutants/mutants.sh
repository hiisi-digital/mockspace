#!/usr/bin/env bash
# Not in the table, because equivalent: dropping the lib.rs, main.rs and src/bin/<name>/main.rs
# roots changes nothing, since those files own their directory by name and the
# orphan walk reaches them first. Only src/bin/<name>.rs needs the root rule.
# Also not in the table, because equivalent: dropping `lifetime_parameter` or `lifetime` from the
# parents whose `name` is renamed with its parameter. The name of a lifetime parameter is a `lifetime`
# node and the identifier inside a `lifetime` has no field, so `leaf` never sees either as an item's name.
# Both survived the run, which is how this was found.
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
a type is grouped by its name alone|single_source.rs|\.entry\(\(def\.name\.as_str\(\), ty\.shape\.as_str\(\)\)\)@@.entry((def.name.as_str(), ""))
the keyword left out of the shape|single_source/shape.rs|if is_noise\(child\) \|\| is_closing_comma\(child\)@@if is_noise(child) || matches!(child.kind(), "struct" | "enum" | "union" | "trait") || is_closing_comma(child)
a semicolon left out of the shape|single_source/shape.rs|if is_noise\(child\) \|\| is_closing_comma\(child\)@@if is_noise(child) || child.kind() == ";" || is_closing_comma(child)
comments kept in the shape|single_source/shape.rs|node\.kind\(\)\.contains\("comment"\)\n( +)\|\| matches!@@false\n\1|| matches!
attributes kept in the shape|single_source/shape.rs|"attribute_item" \| "inner_attribute_item" \| "visibility_modifier"@@"visibility_modifier"
visibilities kept in the shape|single_source/shape.rs|\| "visibility_modifier"@@
a closing comma kept in the shape|single_source/shape.rs|if node\.kind\(\) != ","@@if true
a comment ends the look for the closer|single_source/shape.rs|if !is_noise\(sibling\) \{@@if true {
tuple fields sorted|single_source/shape.rs|"declaration_list"\];@@"declaration_list", "ordered_field_declaration_list"];
members left in the order written|single_source/shape.rs|members\.sort\(\);@@let _ = ();
variants always sorted|single_source/shape.rs|let sorted = !variants_are_numbered_by_place\(node, self\.source\);@@let sorted = true;
a fieldless enum not numbered by place|single_source/shape.rs|fieldless \|\| has_repr\(item, source\)@@has_repr(item, source)
a repr not numbering the variants|single_source/shape.rs|fieldless \|\| has_repr\(item, source\)@@fieldless
hand numbered variants keep their order|single_source/shape.rs|variants\.iter\(\)\.all\(numbered\)@@false
a method body kept in the shape|single_source/shape.rs|"function_item" => node\.child_by_field_name\("body"\) == Some\(child\),@@"function_item" => false,
bounds kept in the order written|single_source/shape.rs|set\.sort\(\);\n( +)set\n@@set
merged bounds kept in the order written|single_source/shape.rs|\.map\(\|\(left, mut set\)\| \{\n( +)set\.sort\(\);@@.map(|(left, mut set)| {
bounds written inline dropped|single_source/shape.rs|bounds\.entry\(name\)\.or_default\(\)\.extend\(set\);@@let _ = (name, set);
bounds in a where clause dropped|single_source/shape.rs|bounds\.entry\(left\)\.or_default\(\)\.extend\(set\);@@let _ = (left, set);
a repeated bound kept|single_source/shape.rs|set\.dedup\(\);@@
generics left as written|single_source/shape.rs|if at_generics && generics\.is_some\(\) \{@@if false {
generic parameters compared by name|single_source/shape.rs|to: format!\("\$\{\}\.\{index\}", self\.depth\)@@to: text.to_string()
scope depth left out of the name|single_source/shape.rs|to: format!\("\$\{\}\.\{index\}", self\.depth\)@@to: format!("\$.{index}")
lifetimes named as types|single_source/shape.rs|"lifetime_parameter" => \(Class::Lifetime,@@"lifetime_parameter" => (Class::Type,
const parameters not renamed|single_source/shape.rs|"const_parameter" => \(Class::Const, param\.child_by_field_name\("name"\)\),@@"const_parameter" => return,
a path segment not told from a type|single_source/shape.rs|"identifier" => class != Class::Lifetime,@@"identifier" => class == Class::Const,
bounded operands kept in the order written|single_source/shape.rs|operands\.sort\(\);@@let _ = ();
a nested bounded type not flattened|single_source/shape.rs|self\.operands\(child, head, out\);@@out.push(self.node(child));
the dyn or impl left off a bounded type|single_source/shape.rs|\*head = Some\(keyword\);@@let _ = keyword;
a name that declares an item renamed|single_source/shape.rs|if names_an_item \{@@if false {
any node with a name field kept as written|single_source/shape.rs|parent\.child_by_field_name\("name"\) == Some\(node\)@@true
a type parameter's own name kept as written|single_source/shape.rs|"type_parameter" \| "lifetime_parameter"@@"lifetime_parameter"
a const parameter's own name kept as written|single_source/shape.rs|\| "const_parameter"@@
a binder closed before the bounds are read|single_source/shape.rs|let binds = left\.kind\(\) == "higher_ranked_trait_bound";@@let binds = false;
a predicate subject read as the type field|single_source/shape.rs|\.filter\(\|_\| binds\)@@
a tuple of one loses its comma|single_source/shape.rs|if node\.parent\(\)\.is_some_and\(is_tuple_of_one\) \{@@if false {
a tuple expression the only tuple of one|single_source/shape.rs|"tuple_type" \| "tuple_expression"@@"tuple_expression"
a comment counted as an element of a tuple|single_source/shape.rs|\.filter\(\|child\| !is_noise\(\*child\)\)\n( +)\.count\(\)@@.count()
the name of a parameter kept|single_source/shape.rs|node\.child_by_field_name\("pattern"\) == Some\(child\)@@false
the mut of a parameter kept|single_source/shape.rs|child\.kind\(\) == "mutable_specifier"@@false
supertraits read as a list of their own|single_source/shape.rs|if node\.kind\(\) == "trait_item" \{@@if false {
any attribute holding repr counts|single_source/shape.rs|\.is_some_and\(\|path\| &source\[path\.byte_range\(\)\] == "repr"\)@@.is_some_and(|_| source[attribute_item.byte_range()].contains("repr"))
TABLE
echo "killed=$killed survived=$survived other=$broken"
