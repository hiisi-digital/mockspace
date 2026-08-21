#!/usr/bin/env python3
"""Does arvo's routine table carry information, or restate the manifest?

Usage: p1_driver_table_is_restatement.py <arvo/mock/benches>

Establishes, for section 4 of the memo:
  - live match arms in routine_for_n vs size rows in bench.toml
  - distinct sections vs distinct (section, bridge type) pairs
    (equal => the type is a function of the section name alone)
  - the warm-container family's arm count and type count

Negative control: if the type varied with the point, the (section,type)
pair count would exceed the section count. It is printed either way so the
check cannot silently pass.
"""
import re, sys, collections, pathlib

root = pathlib.Path(sys.argv[1] if len(sys.argv) > 1 else ".")
main = (root / "src/main.rs").read_text()
toml = (root / "bench.toml").read_text()

arms = re.findall(r'^\s+\("([^"]+)", (\d+)\) => routine_bridge!\((\w+)', main, re.M)
commented = len(re.findall(r'^\s*//.*routine_bridge!', main, re.M))
rows = len(re.findall(r'^\[\[bench\..*\.sizes\]\]', toml, re.M))

print(f"live match arms                : {len(arms)}")
print(f"commented-out arms             : {commented}")
print(f"bench.toml size rows           : {rows}")
print(f"one-to-one                     : {len(arms) == rows}")

sections = {a[0] for a in arms}
pairs = {(a[0], a[2]) for a in arms}
print(f"distinct sections              : {len(sections)}")
print(f"distinct (section, type) pairs : {len(pairs)}")
print(f"type is a function of section  : {len(sections) == len(pairs)}   "
      f"(negative control: False would mean the point selects the type)")
print(f"distinct bridge types          : {len({a[2] for a in arms})}")

fam = collections.Counter(a[0] for a in arms if a[2] == "Case")
print(f"\nwarm-container family (type Case): {len(fam)} sections, {sum(fam.values())} arms")
for k, v in sorted(fam.items()):
    print(f"  {v:>3}  {k}")
