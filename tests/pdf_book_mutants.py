#!/usr/bin/env python3
"""The control for pdf_book_test.sh: break the script, one thing at a time.

Deliberately not named `*_test.sh`, so `./test` does not glob it. It is the
check on that suite rather than part of it, and it is the only thing here that
needs python. Run it by hand after touching either file:

    python3 tests/pdf_book_mutants.py

Every arm in the suite exists to catch one specific way the selection goes
wrong. Nothing in a passing run says whether it does. So each mutation below
puts one of those defects back, runs the suite against the broken copy, and
asserts exactly the arms named fail. A mutation that changes no result means
the arm guarding it is decoration; a mutation that fails an arm not named here
means an arm is catching something other than what it says.

The copy is written to a temporary directory and the suite is pointed at it
through PDF_SH_UNDER_TEST, so the working tree is never modified and a crash
in the middle leaves nothing to clean up.

An earlier version of this ran the mutations through three layers of shell
quoting, and all six anchors silently failed to match. Every mutant was the
unmodified script, every run reported eleven passing, and it read exactly like
a suite that discriminates everything. Hence the anchor assertion below, and
hence this being a file rather than a command someone retypes.
"""

import os
import pathlib
import re
import subprocess
import sys
import tempfile

ROOT = pathlib.Path(__file__).resolve().parent.parent
SCRIPT = ROOT / "scripts" / "pdf.sh"
SUITE = "tests/pdf_book_test.sh"

MARKER_GREP = (
    """    grep -q '^[0-9][0-9]* rows\\. Identifiers are permanent:'"""
    """ "$page" && files+=("$page")"""
)
KEY_SED = (
    """s/^[[:space:]]*key[[:space:]]*=[[:space:]]*"\\([a-z_]*\\)"[[:space:]]*$/\\1/p"""
)

# name -> (what to break, what to break it to, which arms must then fail)
MUTANTS = {
    "the name keeps the dot file's quotes": (
        """    | sed 's/^"//; s/"$//' || true)""",
        """    || true)""",
        {"it_takes_the_name_out_of_a_quoted_digraph_line"},
    ),
    "the name pipeline is fatal again": (
        """ || true)\n[[ -z "$PROJECT_NAME" ]]""",
        """)\n[[ -z "$PROJECT_NAME" ]]""",
        {"it_falls_back_to_the_directory_name_when_the_graph_names_nothing"},
    ),
    "only the config is read": (
        MARKER_GREP,
        """    false && files+=("$page")""",
        {"it_carries_a_registry_page_the_config_never_names"},
    ),
    "only the sweep is read": (
        f'''    done < <(sed -n '{KEY_SED}' "$CONFIG")''',
        """    done < <(true)""",
        {
            "it_carries_the_declared_registry_pages_in_declaration_order",
            "it_carries_a_registry_page_the_config_never_names",
            "it_carries_a_page_that_is_both_declared_and_found_only_once",
            "it_reads_a_key_line_the_config_indents",
        },
    ),
    "the sweep takes every markdown file": (
        MARKER_GREP,
        """    true && files+=("$page")""",
        {
            "it_does_not_carry_a_document_that_is_neither_declared_nor_a_registry_page",
            "it_opens_with_the_design_document_and_closes_with_the_addenda",
        },
    ),
    "a declared page is taken without checking it exists": (
        """        [[ -f "$DOCS_DIR/${upper}.md" ]] && files+=("$DOCS_DIR/${upper}.md")""",
        """        files+=("$DOCS_DIR/${upper}.md")""",
        {"it_carries_nothing_registry_shaped_when_there_are_no_registry_pages"},
    ),
    "the key line must not be indented": (
        KEY_SED,
        """s/^key = "\\([a-z_]*\\)"$/\\1/p""",
        {"it_reads_a_key_line_the_config_indents"},
    ),
}


def failing_arms(script: pathlib.Path) -> set[str]:
    """Which arms failed, having first established that any of them ran.

    An empty set is the answer for a clean run and also the answer when the
    runner never started, and those two are not distinguishable from the
    failures alone. The first version of this passed a stripped PATH, nutshell
    was not on it, every run produced nothing, and the harness reported that
    none of seven mutations was discriminated while the anchors had all
    matched. So the count line is read as proof the suite ran at all.
    """
    env = dict(os.environ, PDF_SH_UNDER_TEST=str(script))
    proc = subprocess.run(
        ["./test", SUITE], cwd=ROOT, env=env, capture_output=True, text=True
    )
    out = proc.stdout + proc.stderr
    ran = re.search(r"(\d+) (?:failed, (\d+) )?passed", out)
    if not ran:
        raise SystemExit(
            "the suite did not run at all:\n" + out.strip()[-2000:]
        )
    return set(re.findall(r"^\[FAIL\] (\w+)", out, re.M))


def main() -> int:
    source = SCRIPT.read_text()

    clean = failing_arms(SCRIPT)
    if clean:
        print(f"the unmutated script already fails: {sorted(clean)}")
        print("nothing below means anything until that is fixed")
        return 2

    bad = 0
    with tempfile.TemporaryDirectory() as tmp:
        target = pathlib.Path(tmp) / "pdf.sh"
        for name, (old, new, want) in MUTANTS.items():
            if old not in source:
                print(f"BROKEN HARNESS: no anchor for [{name}]")
                bad += 1
                continue
            target.write_text(source.replace(old, new))
            target.chmod(0o755)
            got = failing_arms(target)
            if got == want:
                print(f"ok   {name}")
            else:
                bad += 1
                print(f"BAD  {name}")
                for arm in sorted(want - got):
                    print(f"       never failed: {arm}")
                for arm in sorted(got - want):
                    print(f"       also failed:  {arm}")

    print()
    print(f"{len(MUTANTS) - bad} of {len(MUTANTS)} mutations discriminated")
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main())
