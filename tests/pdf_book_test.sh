#!/usr/bin/env nutshell
# shellcheck shell=bash
# =============================================================================
# pdf_book_test - which documents the book is built from, and what it is called
# =============================================================================
# Run: ./test tests/pdf_book_test.sh
#
# Two things decided before pandoc is ever invoked: the project name taken out
# of the dependency graph, and the set of markdown files combined in order. Both
# were wrong in ways nothing reported. The name arrived with the dot file's
# quotes still on it, which pandoc read as a broken YAML title and refused. The
# file set knew about crates and nothing else, so a project whose canon is a
# typed registry got a book with every crate's documentation and none of its
# canon.
#
# Neither failure is visible in a passing run. The second one especially: a book
# missing a chapter is a book, and the only signal is a count nobody compares
# against anything. So the assertions below pin the whole list in order rather
# than checking that some expected file is somewhere in it.
#
# Whether these arms actually discriminate is not something this file can say,
# so it does not claim it. `tests/pdf_book_mutants.py` puts each defect back one
# at a time and asserts exactly the arms named here fail, and it is the thing to
# run after touching either file.
# =============================================================================

use test

# Overridable so the mutation harness can point this suite at a deliberately
# broken copy without touching the working tree. Unset, it is the real script.
PDF_SH="${PDF_SH_UNDER_TEST:-$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)/scripts/pdf.sh}"

if [[ ! -x "$PDF_SH" ]]; then
    printf 'pdf_book_test: %s is not executable\n' "$PDF_SH" >&2
    exit 2
fi

# --- fixtures ----------------------------------------------------------------

# _fixture <root> <digraph-name-line> -> a repo root with a docs/ dir
#
# The dot file is always written, because the script greps it and a missing one
# is a different failure from the ones under test.
_fixture() {
    local root="$1" name_line="$2"
    mkdir -p "$root/docs"
    {
        printf '// AUTO-GENERATED: DO NOT EDIT DIRECTLY\n\n'
        [[ -n "$name_line" ]] && printf '%s\n' "$name_line"
        printf '    rankdir=BT;\n}\n'
    } > "$root/docs/STRUCTURE.GRAPH.dot"
}

# _page <root> <NAME> <rows> -> a generated registry page
#
# The row-count sentence is what mockspace puts on every registry page and on no
# other document, so it is what the script recognises them by. Written out here
# rather than referred to, because a test that generated its fixture with the
# same code it is testing would agree with any mistake in it.
_page() {
    local root="$1" name="$2" rows="$3"
    {
        printf '# %s\n\n' "$name"
        printf '%s rows. Identifiers are permanent: assigned once, never reused, never renumbered.\n\n' "$rows"
        printf '| id | what |\n| --- | --- |\n'
    } > "$root/docs/${name}.md"
}

# _plain <root> <NAME> -> a document that is not a registry page
_plain() {
    printf '# %s\n\nprose.\n' "$2" > "$1/docs/${2}.md"
}

# _config <root> <key>... -> a mockspace.toml declaring those namespaces
_config() {
    local root="$1"; shift
    : > "$root/mockspace.toml"
    for k in "$@"; do
        printf '[[registry.namespace]]\nkey = "%s"\ntitle = "%s"\n\n' "$k" "$k" >> "$root/mockspace.toml"
    done
}

# _name <root> -> the project name the script settled on
_name() {
    "$PDF_SH" --docs-dir "$1/docs" --dry-run 2>/dev/null \
        | sed -n 's/^project : //p'
}

# _files <root> -> the basenames it would combine, in order, space separated
_files() {
    "$PDF_SH" --docs-dir "$1/docs" --dry-run 2>/dev/null \
        | sed -n 's|^file    : .*/||p' \
        | tr '\n' ' ' \
        | sed 's/ $//'
}

# --- the name ----------------------------------------------------------------

#[test]
it_takes_the_name_out_of_a_quoted_digraph_line() {
    # The failure that started this. `digraph "proj" {` yielded `"proj"`, which
    # became `title: ""proj": Design Documentation"` in the metadata, and pandoc
    # stopped at the second quote with a YAML error naming a column.
    local d; d="$(mktemp -d)"
    _fixture "$d/proj" 'digraph "proj" {'
    assert_eq "$(_name "$d/proj")" "proj"
    rm -rf "$d"
}

#[test]
it_still_takes_the_name_from_an_unquoted_digraph_line() {
    # Graphviz quotes a name only when it is not a bare identifier, so both
    # shapes reach this script and stripping the quotes must not eat the
    # unquoted one.
    local d; d="$(mktemp -d)"
    _fixture "$d/proj" 'digraph mockspace {'
    assert_eq "$(_name "$d/proj")" "mockspace"
    rm -rf "$d"
}

#[test]
it_falls_back_to_the_directory_name_when_the_graph_names_nothing() {
    # The fallback is written down in the script and was unreachable: under
    # pipefail a dot file with no digraph line failed the pipeline and `set -e`
    # ended the run one line before the fallback.
    local d; d="$(mktemp -d)"
    _fixture "$d/somerepo" ''
    assert_eq "$(_name "$d/somerepo")" "somerepo"
    rm -rf "$d"
}

# --- the file set ------------------------------------------------------------

#[test]
it_carries_the_declared_registry_pages_in_declaration_order() {
    # Declaration order is the project's own, and alphabetical would put the
    # rulings before the things they rule on.
    local d; d="$(mktemp -d)"
    _fixture "$d/proj" 'digraph "proj" {'
    _config "$d/proj" topic slot ruling
    _page "$d/proj" TOPIC 3
    _page "$d/proj" SLOT 42
    _page "$d/proj" RULING 101
    assert_eq "$(_files "$d/proj")" "TOPIC.md SLOT.md RULING.md"
    rm -rf "$d"
}

#[test]
it_carries_a_registry_page_the_config_never_names() {
    # Some namespaces are mockspace's own and appear in no project's config.
    # Reading only the config left the reference namespace out of every book,
    # which is this section's own defect one namespace narrower, and nothing
    # reported it because a book missing a chapter is still a book.
    local d; d="$(mktemp -d)"
    _fixture "$d/proj" 'digraph "proj" {'
    _config "$d/proj" ruling
    _page "$d/proj" RULING 101
    _page "$d/proj" REFERENCE 1
    assert_eq "$(_files "$d/proj")" "RULING.md REFERENCE.md"
    rm -rf "$d"
}

#[test]
it_carries_a_page_that_is_both_declared_and_found_only_once() {
    # The two passes overlap by design. A page named by the config and carrying
    # the row-count line is reached twice, and must appear once, at the position
    # the config gave it rather than the one the sweep would.
    local d; d="$(mktemp -d)"
    _fixture "$d/proj" 'digraph "proj" {'
    _config "$d/proj" zeta alpha
    _page "$d/proj" ZETA 1
    _page "$d/proj" ALPHA 1
    assert_eq "$(_files "$d/proj")" "ZETA.md ALPHA.md"
    rm -rf "$d"
}

#[test]
it_carries_nothing_registry_shaped_when_there_are_no_registry_pages() {
    # The control for the two tests above. A sweep that picked up every markdown
    # file would satisfy both of them and be entirely wrong, and this is the
    # arm that fails when it does.
    local d; d="$(mktemp -d)"
    _fixture "$d/proj" 'digraph "proj" {'
    _config "$d/proj" ruling topic
    _plain "$d/proj" DESIGN
    _plain "$d/proj" PRINCIPLES
    assert_eq "$(_files "$d/proj")" "DESIGN.md PRINCIPLES.md"
    rm -rf "$d"
}

#[test]
it_does_not_carry_a_document_that_is_neither_declared_nor_a_registry_page() {
    # Same control pointed at one file: a stray document sitting in docs/ is not
    # canon and does not join the book by being uppercase.
    local d; d="$(mktemp -d)"
    _fixture "$d/proj" 'digraph "proj" {'
    _config "$d/proj" ruling
    _page "$d/proj" RULING 2
    _plain "$d/proj" NOTES
    assert_eq "$(_files "$d/proj")" "RULING.md"
    rm -rf "$d"
}

#[test]
it_reads_a_key_line_the_config_indents() {
    # TOML permits the indentation and mockspace does not forbid it, so a config
    # that uses it must not silently lose its declared order and fall through to
    # the alphabetical sweep.
    local d; d="$(mktemp -d)"
    _fixture "$d/proj" 'digraph "proj" {'
    {
        printf '[[registry.namespace]]\n    key = "zeta"\n\n'
        printf '[[registry.namespace]]\n    key = "alpha"\n\n'
    } > "$d/proj/mockspace.toml"
    _page "$d/proj" ZETA 1
    _page "$d/proj" ALPHA 1
    assert_eq "$(_files "$d/proj")" "ZETA.md ALPHA.md"
    rm -rf "$d"
}

#[test]
it_opens_with_the_design_document_and_closes_with_the_addenda() {
    # The canon sits between the entrypoint and the appendices. Putting it after
    # the workflow addendum would read as an afterthought, and before the design
    # document as the opening chapter.
    local d; d="$(mktemp -d)"
    _fixture "$d/proj" 'digraph "proj" {'
    _config "$d/proj" ruling
    _plain "$d/proj" DESIGN
    _page  "$d/proj" RULING 7
    _plain "$d/proj" STRUCTURE
    _plain "$d/proj" PRINCIPLES
    _plain "$d/proj" WORKFLOW
    assert_eq "$(_files "$d/proj")" \
        "DESIGN.md RULING.md STRUCTURE.md PRINCIPLES.md WORKFLOW.md"
    rm -rf "$d"
}

#[test]
it_needs_no_latex_engine_to_say_what_it_would_build() {
    # The dry run exists to be checkable on a machine with no TeX distribution,
    # and every other test here would pass on a machine that happens to have
    # one, so none of them measures this. The PATH below holds the plain tools
    # the script reaches for and nothing else: no xelatex, no lualatex, no
    # pdflatex, no tectonic, no pandoc.
    local d; d="$(mktemp -d)"
    local bin="$d/bin"
    mkdir -p "$bin"
    local t
    for t in bash grep sed tr basename dirname ls cat mktemp date; do
        local real; real="$(type -P "$t" || true)"
        [[ -n "$real" ]] && ln -sf "$real" "$bin/$t"
    done

    _fixture "$d/proj" 'digraph "proj" {'
    _plain "$d/proj" DESIGN

    # The control for the control: if any engine were reachable on this PATH the
    # arm below would pass without testing anything.
    assert_fails env PATH="$bin" "$bin/bash" -c 'command -v xelatex || command -v lualatex || command -v pdflatex || command -v tectonic'

    assert_ok env PATH="$bin" "$bin/bash" "$PDF_SH" --docs-dir "$d/proj/docs" --dry-run
    rm -rf "$d"
}
