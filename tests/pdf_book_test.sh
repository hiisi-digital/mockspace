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
# file set once chose registry pages by a line each carried; the registry writes
# none now, and the set is pinned so that choice does not return unnoticed.
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

# _page <root> <NAME> <rows> -> a registry page as an earlier version wrote it
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
it_opens_with_the_design_document_and_closes_with_the_addenda() {
    # The canon sits between the entrypoint and the appendices. Putting it after
    # the workflow addendum would read as an afterthought, and before the design
    # document as the opening chapter.
    local d; d="$(mktemp -d)"
    _fixture "$d/proj" 'digraph "proj" {'
    _plain "$d/proj" DESIGN
    _plain "$d/proj" STRUCTURE
    _plain "$d/proj" PRINCIPLES
    _plain "$d/proj" WORKFLOW
    assert_eq "$(_files "$d/proj")" \
        "DESIGN.md STRUCTURE.md PRINCIPLES.md WORKFLOW.md"
    rm -rf "$d"
}

#[test]
it_carries_no_registry_page_even_when_one_sits_in_docs() {
    # The registry writes no page, so the book has no chapter for one. A page an
    # earlier version left behind, declared in the config and carrying the
    # row-count line, is not picked up: the case that fails is the old selection,
    # which carried it.
    local d; d="$(mktemp -d)"
    _fixture "$d/proj" 'digraph "proj" {'
    _config "$d/proj" ruling
    _page "$d/proj" RULING 101
    _plain "$d/proj" DESIGN
    assert_eq "$(_files "$d/proj")" "DESIGN.md"
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
