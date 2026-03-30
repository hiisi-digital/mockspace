#!/usr/bin/env bash
# mockspace-pdf — generate a design documentation PDF from docs/
#
# Reusable across any repo that uses mockspace and has generated docs/.
# Requires: pandoc, xelatex (or pdflatex for --engine pdflatex)
#
# Usage:
#   ./pdf.sh [OPTIONS]
#
# Options:
#   --docs-dir <path>   Path to docs/ directory (auto-detected if omitted)
#   --out <file>        Output PDF path (default: <project>-design.pdf in repo root)
#   --title <name>      Document title override
#   --open              Open the PDF after generation (macOS: open, Linux: xdg-open)
#   --no-toc            Skip table of contents
#   --engine <engine>   PDF engine: xelatex (default) or pdflatex

set -euo pipefail

# ─── defaults ─────────────────────────────────────────────────────────────────

DOCS_DIR=""
OUT_FILE=""
TITLE=""
OPEN_AFTER=false
WITH_TOC=true
PDF_ENGINE=""  # auto-detected below

# ─── argument parsing ─────────────────────────────────────────────────────────

while [[ $# -gt 0 ]]; do
    case $1 in
        --docs-dir) DOCS_DIR="$2"; shift 2 ;;
        --out)      OUT_FILE="$2"; shift 2 ;;
        --title)    TITLE="$2"; shift 2 ;;
        --open)     OPEN_AFTER=true; shift ;;
        --no-toc)   WITH_TOC=false; shift ;;
        --engine)   PDF_ENGINE="$2"; shift 2 ;;  # xelatex, pdflatex, lualatex, tectonic
        *) echo "error: unknown option: $1" >&2; exit 1 ;;
    esac
done

# ─── locate docs dir ──────────────────────────────────────────────────────────

if [[ -z "$DOCS_DIR" ]]; then
    dir="$(pwd)"
    while [[ "$dir" != "/" ]]; do
        if [[ -f "$dir/docs/STRUCTURE.GRAPH.dot" ]]; then
            DOCS_DIR="$dir/docs"
            REPO_ROOT="$dir"
            break
        fi
        dir="$(dirname "$dir")"
    done
    if [[ -z "$DOCS_DIR" ]]; then
        echo "error: no docs/STRUCTURE.GRAPH.dot found." >&2
        echo "       run from within a mockspace repo, or use --docs-dir." >&2
        exit 1
    fi
else
    DOCS_DIR="$(cd "$DOCS_DIR" && pwd)"
    REPO_ROOT="$(dirname "$DOCS_DIR")"
fi

DOT_FILE="$DOCS_DIR/STRUCTURE.GRAPH.dot"

# ─── auto-detect PDF engine if not specified ──────────────────────────────────

if [[ -z "$PDF_ENGINE" ]]; then
    for candidate in xelatex lualatex pdflatex tectonic; do
        if command -v "$candidate" &>/dev/null; then
            PDF_ENGINE="$candidate"
            break
        fi
    done
    if [[ -z "$PDF_ENGINE" ]]; then
        echo "error: no LaTeX PDF engine found." >&2
        echo "       install one of: xelatex, lualatex, pdflatex, tectonic" >&2
        echo "       macOS:  brew install tectonic  (or mactex for the full suite)" >&2
        echo "       linux:  apt install texlive-xetex" >&2
        exit 1
    fi
fi

# ─── extract project name from the DOT file ───────────────────────────────────

PROJECT_NAME=$(grep -m1 '^digraph ' "$DOT_FILE" \
    | sed 's/^digraph[[:space:]]*//' \
    | sed 's/[[:space:]]*{.*//')
[[ -z "$PROJECT_NAME" ]] && PROJECT_NAME="$(basename "$REPO_ROOT")"

[[ -z "$TITLE"    ]] && TITLE="$PROJECT_NAME — Design Documentation"
[[ -z "$OUT_FILE" ]] && OUT_FILE="$REPO_ROOT/$PROJECT_NAME-design.pdf"

echo "project : $PROJECT_NAME"
echo "docs    : $DOCS_DIR"
echo "output  : $OUT_FILE"

# ─── extract crate order from DOT depth groups ────────────────────────────────
# The DOT file contains lines like:
#   { rank=same; loimu_id; loimu_signal; } // depth N
# These are emitted in topological depth order by mockspace, so file order == depth order.

ordered_crates=()
while IFS= read -r line; do
    # Strip everything before and after the node list
    inner="${line#*rank=same;}"
    inner="${inner%%\}*}"
    for token in $inner; do
        node="${token%%;}"
        [[ -n "$node" ]] && ordered_crates+=("$node")
    done
done < <(grep 'rank=same' "$DOT_FILE")

# ─── build ordered file list ──────────────────────────────────────────────────

files=()

# Entrypoint: the root design doc opens the document
[[ -f "$DOCS_DIR/DESIGN.md" ]] && files+=("$DOCS_DIR/DESIGN.md")

# Per-crate docs in dependency depth order.
# Node name loimu_behavior_macros → prefix LOIMU_BEHAVIOR_MACROS.
# Overview file first, then remaining deep-dive files alphabetically.
for crate_node in "${ordered_crates[@]}"; do
    prefix="${crate_node^^}"  # bash uppercase expansion
    overview="$DOCS_DIR/${prefix}_OVERVIEW.md"
    [[ -f "$overview" ]] && files+=("$overview")
    for f in "$DOCS_DIR/${prefix}_"*.md; do
        [[ "$f" == "$overview" ]] && continue
        [[ -f "$f" ]] && files+=("$f")
    done
done

# Reference appendices
for footer in STRUCTURE.md DESIGN-DEEP-DIVES.md; do
    [[ -f "$DOCS_DIR/$footer" ]] && files+=("$DOCS_DIR/$footer")
done

# Addenda: principles and workflow come last
for addendum in PRINCIPLES.md WORKFLOW.md; do
    [[ -f "$DOCS_DIR/$addendum" ]] && files+=("$DOCS_DIR/$addendum")
done

# Deduplicate, preserving first-seen order
declare -A _seen
unique_files=()
for f in "${files[@]}"; do
    if [[ -z "${_seen[$f]+x}" ]]; then
        _seen[$f]=1
        unique_files+=("$f")
    fi
done

echo "files   : ${#unique_files[@]} markdown files"

# ─── preprocess: patch SVG image refs → PNG (LaTeX cannot embed SVG) ──────────

WORK_DIR="$(mktemp -d)"
trap 'rm -rf "$WORK_DIR"' EXIT

patched_files=()
for f in "${unique_files[@]}"; do
    dest="$WORK_DIR/$(basename "$f")"
    bname="$(basename "$f")"

    # For DESIGN.md: strip ## sections that exist as standalone addenda at the end
    # (Principles, Workflow) to avoid duplication and the appearance of them being first.
    if [[ "$bname" == "DESIGN.md" ]]; then
        awk '
            /^## Principles/ { skip=1; next }
            /^## Workflow/   { skip=1; next }
            skip && /^## /   { skip=0 }
            !skip            { print }
        ' "$f" \
        | sed 's/\(!\[[^]]*\]([^)]*\)\.svg)/\1.png)/g' > "$dest"
    else
        # Replace .svg in image links with .png (LaTeX cannot embed SVG)
        sed 's/\(!\[[^]]*\]([^)]*\)\.svg)/\1.png)/g' "$f" > "$dest"
    fi
    patched_files+=("$dest")
done

# ─── inject embedded figures for image-only hyperlinks ───────────────────────
# When an image only appears as a hyperlink ([text](img.svg)) and never as an
# embed (![alt](img.png)), pandoc emits no Figure block and therefore no LaTeX
# \label.  \hyperref[anchor]{text} then resolves to nothing and the link
# disappears silently.  Fix: inject the figure embed right after the first
# hyperlink occurrence so the label exists for the cross-reference to land on.

for img in "$DOCS_DIR"/*.png; do
    [[ -f "$img" ]] || continue
    stem="$(basename "$img" .png)"

    for f in "${patched_files[@]}"; do
        # Already embedded as a figure in this file — nothing to do
        if grep -qE '!\[[^]]*\]\('"$stem"'\.png\)' "$f" 2>/dev/null; then
            continue
        fi
        # Has a hyperlink to stem.svg or stem.png but no embed — inject once
        if grep -qF "](${stem}.svg)" "$f" || grep -qF "](${stem}.png)" "$f"; then
            awk -v stem="$stem" '
            {
                print
                if (!done && index($0, "![") == 0) {
                    if (index($0, "](" stem ".svg)") > 0 \
                     || index($0, "](" stem ".png)") > 0) {
                        print ""
                        print "![](" stem ".png)"
                        print ""
                        done = 1
                    }
                }
            }
            ' "$f" > "${f}.inj" && mv "${f}.inj" "$f"
            break  # inject into the first file that references this image
        fi
    done
done

# ─── build heading map for cross-file link resolution ────────────────────────
# Pandoc doesn't resolve cross-file .md links when combining files into one PDF.
# We build a map of filename.md → first-heading-anchor, then emit a Lua filter
# that rewrites those links to internal hyperlinks at pandoc parse time.

heading_to_anchor() {
    # Approximate pandoc's identifier generation:
    # lowercase, spaces→dashes, strip non-alnum except dash/underscore/period,
    # drop leading non-alpha, collapse repeated dashes.
    local h="${1,,}"
    h="${h// /-}"
    h="$(echo "$h" | tr -dc '[:alnum:]_.-')"
    h="${h#"${h%%[[:alpha:]]*}"}"
    h="$(echo "$h" | sed 's/--*/-/g')"
    echo "$h"
}

declare -A _heading_map
for f in "${unique_files[@]}"; do
    bname="$(basename "$f")"
    first_h=$(grep -m1 '^##* ' "$f" | sed 's/^##* //')
    [[ -n "$first_h" ]] && _heading_map["$bname"]="$(heading_to_anchor "$first_h")"
done

# Build an image anchor map: stem → "img-slug"
# This lets .svg/.png hyperlinks target the embedded figure in the PDF.
# Keyed by both the original stem and the .svg basename for easy lookup.
img_to_anchor() {
    local base="$1"
    echo "img-$(echo "$base" | tr '[:upper:]' '[:lower:]' | tr -cs '[:alnum:]' '-' | sed 's/--*/-/g; s/-$//')"
}

declare -A _img_map
for img in "$DOCS_DIR"/*.png; do
    [[ -f "$img" ]] || continue
    base="$(basename "$img" .png)"
    anchor="$(img_to_anchor "$base")"
    _img_map["$base"]="$anchor"          # STRUCTURE.GRAPH → img-structure-graph
    _img_map["${base}.svg"]="$anchor"    # STRUCTURE.GRAPH.svg → same anchor
    _img_map["${base}.png"]="$anchor"    # STRUCTURE.GRAPH.png → same anchor
done

LUA_FILTER="$WORK_DIR/_links.lua"
{
    printf 'local heading_map = {\n'
    for key in "${!_heading_map[@]}"; do
        printf '  ["%s"] = "%s",\n' "$key" "${_heading_map[$key]}"
    done
    printf '}\n\n'

    printf 'local img_map = {\n'
    for key in "${!_img_map[@]}"; do
        printf '  ["%s"] = "%s",\n' "$key" "${_img_map[$key]}"
    done
    printf '}\n\n'
    cat <<'LUAEOF'
-- Strip file extensions (.md, .svg, .png, .dot, etc.) from display text Strs,
-- and any leading path components, so links read as clean labels.
local function clean_link_text(inlines)
    for _, inline in ipairs(inlines) do
        if inline.t == 'Str' then
            local txt = inline.text
            if txt:match('%.[%a]+$') then
                txt = txt:match('[^/\\]+$') or txt   -- drop path prefix
                txt = txt:gsub('%.[%a]+$', '')        -- drop extension
                inline.text = txt
            end
        end
    end
    return inlines
end

function Link(el)
    local target = el.target

    -- Pass absolute URIs through; still clean their display text
    if target:match('^%a[%a%d+%-%.]*/') or target:match('^%a[%a%d+%-%.]*:') then
        el.content = clean_link_text(el.content)
        return el
    end

    -- Image/binary file links (.svg, .png, .dot, .jpg …)
    -- If the image is embedded in this PDF as a figure, link to its anchor.
    -- Otherwise strip the link entirely (no dead external reference).
    local _img_exts = {svg=true, png=true, dot=true, jpg=true, jpeg=true, gif=true, webp=true}
    local _ext = target:match('%.(%a+)$')
    if _ext and _img_exts[_ext:lower()] then
        local fname_only = target:match('[^/\\]+$') or target
        local anchor = img_map[fname_only]
        if anchor then
            el.target = '#' .. anchor
            el.content = clean_link_text(el.content)
            return el
        end
        return clean_link_text(el.content)
    end

    -- FILENAME.md#fragment  →  #fragment
    local frag = target:match('[^/\\]+%.md#(.+)$')
    if frag then
        el.target = '#' .. frag
        el.content = clean_link_text(el.content)
        return el
    end

    -- FILENAME.md  →  first-heading anchor for that file
    local fname = target:match('[^/\\]+%.md$')
    if fname then
        -- Some templates emit LOIMU-FOO.md (dash separator) while generated
        -- files are named LOIMU_FOO.md (underscore); normalize before lookup.
        local anchor = heading_map[fname] or heading_map[fname:gsub('%-', '_')]
        if anchor and anchor ~= '' then
            el.target = '#' .. anchor
        else
            local slug = fname:gsub('%.md$', ''):lower():gsub('[^%w%-]', '-'):gsub('%-+', '-')
            el.target = '#' .. slug
        end
        el.content = clean_link_text(el.content)
        return el
    end

    -- Everything else: just clean display text
    el.content = clean_link_text(el.content)
    return el
end

-- Tag each figure block with a stable identifier so pandoc emits \label{anchor}
-- in LaTeX. Pandoc renders [text](#anchor) as \hyperref[anchor]{text}, which
-- resolves via \label — so this is the correct mechanism (not \hypertarget).
-- Must use Figure (block), NOT Image (inline): setting the inline identifier
-- has no effect on the wrapping figure's \label.
function Figure(el)
    if el.attr.identifier ~= '' then return el end  -- already identified
    -- Walk the figure's block content to find the first Image src
    for _, block in ipairs(el.content) do
        if block.t == 'Plain' or block.t == 'Para' then
            for _, inline in ipairs(block.content) do
                if inline.t == 'Image' then
                    local fname = inline.src:match('[^/\\]+$') or inline.src
                    local anchor = img_map[fname]
                    if anchor then
                        el.attr = pandoc.Attr(anchor, el.attr.classes, el.attr.attributes)
                        return el
                    end
                end
            end
        end
    end
    return el
end
LUAEOF
} > "$LUA_FILTER"

# ─── write pandoc metadata and LaTeX header ───────────────────────────────────

META="$WORK_DIR/_meta.yaml"
cat > "$META" <<YAML
---
title: "$TITLE"
date: "$(date '+%Y-%m-%d')"
toc-depth: 3
number-sections: true
colorlinks: true
linkcolor: NavyBlue
urlcolor: NavyBlue
geometry: "left=1.3in, right=1.3in, top=1.4in, bottom=1.2in"
fontsize: 11pt
linestretch: 1.25
highlight-style: tango
---
YAML

# Auto-detect the best available fonts for Unicode coverage.
# Priority: Nerd Font variants (box-drawing, symbols) > broad Unicode > common defaults.
pick_font() {
    local candidates=("$@")
    if ! command -v fc-list &>/dev/null; then echo ""; return; fi
    for name in "${candidates[@]}"; do
        if fc-list | grep -qi "^[^:]*:[^:]*${name}"; then
            echo "$name"; return
        fi
    done
    echo ""
}

MAIN_FONT=$(pick_font \
    "Source Sans Pro" \
    "Liberation Sans" \
    "DejaVu Sans" \
    "Helvetica Neue" \
    "Arial Unicode MS" \
    "Georgia")

MONO_FONT=$(pick_font \
    "JetBrainsMono Nerd Font" \
    "JetBrainsMonoNL Nerd Font Mono" \
    "Hack Nerd Font" \
    "DejaVu Sans Mono" \
    "Liberation Mono" \
    "Menlo" \
    "Courier New")

# Write the LaTeX header separately to avoid YAML backslash escaping issues.
LATEX_HDR="$WORK_DIR/_header.tex"

# Only use fontspec when a Unicode engine is being used (not pdflatex)
FONT_LINES=""
if [[ "$PDF_ENGINE" != "pdflatex" ]] && { [[ -n "$MAIN_FONT" ]] || [[ -n "$MONO_FONT" ]]; }; then
    FONT_LINES+="\\usepackage{fontspec}\n"
    [[ -n "$MAIN_FONT" ]] && FONT_LINES+="\\setmainfont{${MAIN_FONT}}[Ligatures=TeX]\n"
    [[ -n "$MAIN_FONT" ]] && FONT_LINES+="\\setsansfont{${MAIN_FONT}}[Ligatures=TeX]\n"
    [[ -n "$MONO_FONT" ]] && FONT_LINES+="\\setmonofont{${MONO_FONT}}[Scale=0.9]\n"
fi

printf '%b' "$FONT_LINES" > "$LATEX_HDR"
cat >> "$LATEX_HDR" <<LATEX
\usepackage{fancyhdr}
\pagestyle{fancy}
\fancyhf{}
\fancyhead[L]{\small\textit{${PROJECT_NAME}}}
\fancyhead[R]{\small\textit{Design Documentation}}
\fancyfoot[C]{\small\thepage}
\renewcommand{\headrulewidth}{0.3pt}
\renewcommand{\footrulewidth}{0pt}
LATEX

# ─── run pandoc ───────────────────────────────────────────────────────────────

pandoc_args=(
    "$META"
    "${patched_files[@]}"
    --from "markdown+fenced_code_blocks+pipe_tables+smart+autolink_bare_uris"
    --pdf-engine "$PDF_ENGINE"
    --resource-path "$DOCS_DIR"
    --include-in-header "$LATEX_HDR"
    --lua-filter "$LUA_FILTER"
    --output "$OUT_FILE"
)
$WITH_TOC && pandoc_args+=(--toc --toc-depth=3)


echo "running pandoc (engine: $PDF_ENGINE)..."
PANDOC_LOG="$WORK_DIR/_pandoc.log"
if pandoc "${pandoc_args[@]}" 2>"$PANDOC_LOG"; then
    # Suppress expected LaTeX noise for technical docs:
    # - hbox over/underfull (paragraph justification)
    # - Missing character warnings for any remaining unrepresented Unicode
    grep -Ev '(Overfull|Underfull).*hbox|warnings were issued|Missing character|could not represent character|you may need to load|choose a different font|warning: texput\.[^:]+:[0-9]+:[[:space:]]*$' \
        "$PANDOC_LOG" >&2 || true
else
    cat "$PANDOC_LOG" >&2
    echo "error: pandoc failed" >&2
    exit 1
fi

echo "done    : $OUT_FILE"

if $OPEN_AFTER; then
    case "$(uname -s)" in
        Darwin) open "$OUT_FILE" ;;
        Linux)  xdg-open "$OUT_FILE" ;;
    esac
fi
