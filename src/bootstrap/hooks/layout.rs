//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! What the generated hooks are told about the project: the layout they read, and
//! the shell that carries it.

use super::*;

/// What the generated hooks are told about the project: where its source lives,
/// what a nuked crate carries, and which lint packs it declares by path.
///
/// All of it comes from the same places the engine reads it. The directories are
/// those `SrcLayout` holds, so a hook and a lint address a crate by one
/// definition, and the marker is the config's `nuke_marker`, the text the
/// engine's own nuke detection looks for.
pub(crate) struct HookLayout {
    pub(crate) src_dirs:    Vec<String>,
    pub(crate) nuke_marker: String,
    /// The directories of the `[lint-crates]` packs declared by path, relative
    /// to the repository root. A pack is code the run compiles, as a file under
    /// the mock directory's `lints/` is.
    pub(crate) lint_packs:  Vec<String>,
}

impl HookLayout {
    /// From the absolute directories the config carries, as `SrcLayout` reads
    /// them, and the path packs `config_path` declares under `[lint-crates]`.
    pub(crate) fn declared(
        repo_root: &Path,
        mock_dir: &Path,
        src_dirs: &[PathBuf],
        nuke_marker: &str,
        config_path: &Path,
    ) -> Self {
        let layout = mockspace_lint_rules::src_layout::SrcLayout::new(mock_dir, src_dirs);
        Self {
            src_dirs:    layout.dirs().to_vec(),
            nuke_marker: nuke_marker.to_string(),
            lint_packs:  lint_pack_dirs(repo_root, mock_dir, config_path),
        }
    }

    /// From directories already relative to the mock directory, with the
    /// marker the config defaults to.
    #[cfg(test)]
    pub(crate) fn of(src_dirs: &[&str]) -> Self {
        Self {
            src_dirs:    src_dirs.iter().map(|d| d.to_string()).collect(),
            nuke_marker: crate::config::DEFAULT_NUKE_MARKER.to_string(),
            lint_packs:  Vec::new(),
        }
    }

    /// As `of`, with path packs at these repository-relative directories.
    #[cfg(test)]
    pub(crate) fn of_with_packs(src_dirs: &[&str], packs: &[&str]) -> Self {
        Self {
            lint_packs: packs.iter().map(|d| d.to_string()).collect(),
            ..Self::of(src_dirs)
        }
    }
}

/// The directories of the `[lint-crates]` packs `config_path` declares by path,
/// relative to `repo_root`.
///
/// A path is written as cargo reads it from the generated lint crate, which
/// sits at `build_dir::lint_crate_dir`, so it is resolved from there.
/// A pack outside the repository cannot be staged and is left out.
fn lint_pack_dirs(repo_root: &Path, mock_dir: &Path, config_path: &Path) -> Vec<String> {
    let from = crate::build_dir::lint_crate_dir(mock_dir);
    let mut dirs = Vec::new();
    for (name, spec) in super::lints::parse_lint_crates(config_path) {
        // `parse_lint_crates` re-emits each dependency as the value cargo reads, so
        // the spec parses as the right-hand side of a key whatever form it was
        // written in.
        let Ok(doc) = format!("{name} = {spec}").parse::<toml_edit::DocumentMut>() else {
            continue;
        };
        let Some(path) = doc
            .get(&name)
            .and_then(|dep| dep.get("path"))
            .and_then(|p| p.as_str())
        else {
            continue;
        };
        let resolved = lexically_normal(&from.join(path));
        let Ok(rel) = resolved.strip_prefix(repo_root) else {
            continue;
        };
        let rel = rel.to_string_lossy().replace('\\', "/");
        if !rel.is_empty() && !dirs.contains(&rel) {
            dirs.push(rel);
        }
    }
    dirs
}

/// `path` with its `.` and `..` components folded away, without touching the
/// disk: a pack's directory is resolved the way cargo reads the path, and a
/// symlink on the way is not followed by either side.
fn lexically_normal(path: &Path) -> PathBuf {
    use std::path::Component;
    let mut out = PathBuf::new();
    for c in path.components() {
        match c {
            Component::CurDir => {},
            Component::ParentDir => {
                out.pop();
            },
            other => out.push(other),
        }
    }
    out
}

/// Single-quote a word for bash.
fn sh_quote(word: &str) -> String {
    format!("'{}'", word.replace('\'', "'\\''"))
}

/// The shell both generated hooks share for reading the project's source layout.
///
/// Written after `MOCK_DIR` is set. `SRC_DIRS` is the declaration, and the
/// functions are the only places a hook decides where a crate lives: which
/// crate a path belongs to, whether a path is source, which workspace a
/// manifest or lockfile belongs to, whether the workspace is nuked, and the
/// pathspecs to hand git. An empty declaration means no path is source and no
/// crate exists, which is what a documentation repository is.
pub(crate) fn src_layout_shell(layout: &HookLayout) -> String {
    let words: Vec<String> = layout.src_dirs.iter().map(|d| sh_quote(d)).collect();
    let packs: Vec<String> = layout
        .lint_packs
        .iter()
        .map(|d| sh_quote(&format!("{d}/")))
        .collect();
    format!(
        r##"# The declared source directories, relative to the mock directory.
SRC_DIRS=({dirs})

# The directories of the lint packs the config declares by path, relative to the
# repository root.
MS_PACKS=({packs})

# What a nuked crate's `lib.rs` carries, as the config names it.
NUKE_MARKER={marker}

# The crates, comma-joined and sorted, that the newline-separated paths in $1
# belong to: the directory directly under whichever source directory holds the
# path. A path directly in a source directory belongs to no crate.
ms_crates_of() {{
    local p d rest
    while IFS= read -r p; do
        for d in ${{SRC_DIRS[@]+"${{SRC_DIRS[@]}}"}}; do
            if [[ "$p" == "$MOCK_DIR/$d/"* ]]; then
                rest="${{p#"$MOCK_DIR/$d/"}}"
                case "$rest" in
                    */*) printf '%s\n' "${{rest%%/*}}" ;;
                esac
                break
            fi
        done
    done <<< "$1" | sort -u | tr '\n' ',' | sed 's/,$//'
}}

# The directory of the Cargo workspace that holds source directory $1: the
# nearest one at or above it, up to the mock directory, whose Cargo.toml has a
# `[workspace]` table. The mock directory is the answer when none does.
ms_ws_root_of() {{
    local w="$MOCK_DIR/$1"
    while [ "$w" != "$MOCK_DIR" ] && [ "$w" != "." ] && [ "$w" != "/" ]; do
        if grep -q '^\[workspace' "$w/Cargo.toml" 2>/dev/null; then
            printf '%s\n' "$w"
            return 0
        fi
        w="$(dirname "$w")"
    done
    printf '%s\n' "$MOCK_DIR"
}}

# The names of the crates directly under source directory $1, one a line. A crate
# is what the engine's `package_dirs_in` lists, a directory directly under a
# source directory that is not a symlink, with the dot-directories among them, so
# the glob takes dot-names and a link is passed over. The one place a hook lists
# crates from disk. A subshell, so the option stays local.
ms_package_dirs() (
    shopt -s dotglob
    local c
    for c in "$MOCK_DIR/$1/"*/; do
        if [ -d "$c" ] && [ ! -L "${{c%/}}" ]; then
            basename "$c"
        fi
    done
)

# The crates, comma-joined and sorted, of every workspace whose manifest or
# lockfile is among the newline-separated paths in $1: all the crates under the
# source directories that workspace holds. A lockfile moves every crate in its
# own workspace and none in another.
ms_ws_crates_of() {{
    local d w
    for d in ${{SRC_DIRS[@]+"${{SRC_DIRS[@]}}"}}; do
        w="$(ms_ws_root_of "$d")"
        if grep -qxF -e "$w/Cargo.toml" -e "$w/Cargo.lock" <<< "$1"; then
            ms_package_dirs "$d"
        fi
    done | sort -u | tr '\n' ',' | sed 's/,$//'
}}

# The comma-joined crate lists in the arguments as one sorted list, no repeats.
ms_union() {{
    printf '%s\n' "$@" | tr ',' '\n' | sed '/^$/d' | sort -u | tr '\n' ',' | sed 's/,$//'
}}

# The newline-separated paths in $1 that sit under a source directory and end in $2.
ms_src_files_of() {{
    local p d
    while IFS= read -r p; do
        for d in ${{SRC_DIRS[@]+"${{SRC_DIRS[@]}}"}}; do
            if [[ "$p" == "$MOCK_DIR/$d/"*"$2" ]]; then
                printf '%s\n' "$p"
                break
            fi
        done
    done <<< "$1"
}}

# Whether every crate under the source directories carries the nuke marker, and
# there is at least one. The engine's `detect_nuked_workspace` is the same
# question: one live crate anywhere means the workspace is not nuked. The crates
# are those `ms_package_dirs` lists.
ms_nuked() {{
    local d c any=1
    for d in ${{SRC_DIRS[@]+"${{SRC_DIRS[@]}}"}}; do
        while IFS= read -r c; do
            any=0
            if ! grep -qF -- "$NUKE_MARKER" "$MOCK_DIR/$d/$c/src/lib.rs" 2>/dev/null; then
                return 1
            fi
        done < <(ms_package_dirs "$d")
    done
    return $any
}}

# The code the run compiles besides the crates it lints: the project lints and
# the lint packs declared by path. A pack under a source directory is also a
# crate by its place, and this is what keeps the crate reading from bounding a
# commit to it, since a changed lint has to run over every crate.
MS_CODE=("$MOCK_DIR/lints/" ${{MS_PACKS[@]+"${{MS_PACKS[@]}}"}})

# The inputs every crate shares: the registry, the code above and the config. A
# change to one reaches all of them, so it is never bounded to the crates a
# commit or push also touches. The config is named at the two places it lives,
# the repo root and the mock directory, and nowhere deeper: a fixture of that
# name under a source directory belongs to its crate. A manifest or lockfile is
# not here: it reaches the crates of its own workspace, which `ms_ws_crates_of`
# answers.
MS_SHARED=("$MOCK_DIR/registry/" "${{MS_CODE[@]}}" "mockspace.toml" "$MOCK_DIR/mockspace.toml")
"##,
        dirs = words.join(" "),
        packs = packs.join(" "),
        marker = sh_quote(&layout.nuke_marker),
    )
}
