#![allow(unused_imports)]
use super::*;

/// Parse the `[lint-crates]` section from mockspace.toml.
///
/// Returns a list of (crate_name, cargo_dep_spec_as_toml_string) pairs in
/// declaration order. Each value is re-emitted verbatim into the proxy's
/// Cargo.toml so any cargo-accepted dep form works: `"0.1"`, `{ path = ... }`,
/// `{ git = ..., branch = ... }`, etc.
///
/// Returns empty vec if mockspace.toml is missing, unparseable, or has no
/// `[lint-crates]` section.
pub(crate) fn parse_lint_crates(mockspace_toml: &Path) -> Vec<(String, String)> {
    let content = match fs::read_to_string(mockspace_toml) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };

    let doc = match content.parse::<toml_edit::DocumentMut>() {
        Ok(d) => d,
        Err(_) => return Vec::new(),
    };

    let section = match doc.get("lint-crates").and_then(|i| i.as_table()) {
        Some(t) => t,
        None => return Vec::new(),
    };

    let mut result = Vec::new();
    for (name, item) in section.iter() {
        // Value form (string like "0.1" or inline table `{ path = ... }`).
        if let Some(v) = item.as_value() {
            result.push((name.to_string(), v.to_string().trim().to_string()));
            continue;
        }
        // Sub-table form: [lint-crates.foo]\n path = "..."
        if let Some(tbl) = item.as_table() {
            // Re-emit as an inline table so it fits on the [dependencies] line.
            let mut inline = toml_edit::InlineTable::new();
            for (k, v) in tbl.iter() {
                if let Some(val) = v.as_value() {
                    inline.insert(k, val.clone());
                }
            }
            result.push((name.to_string(), inline.to_string().trim().to_string()));
        }
    }
    result
}


/// Discover `.rs` files in the custom lints directory.
/// Returns a sorted list of file stems (e.g., "my_lint" from "my_lint.rs").
pub(crate) fn discover_custom_lint_files(lints_dir: &Path) -> Vec<String> {
    let mut files = Vec::new();
    if !lints_dir.is_dir() {
        return files;
    }

    if let Ok(entries) = fs::read_dir(lints_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().map(|e| e == "rs").unwrap_or(false) {
                if let Some(stem) = path.file_stem() {
                    let stem_str = stem.to_string_lossy().to_string();
                    if is_valid_rust_ident(&stem_str) {
                        files.push(stem_str);
                    } else {
                        eprintln!(
                            "warning: skipping custom lint file `{}`: stem `{}` is not a valid Rust identifier (only [a-z0-9_] allowed)",
                            path.display(),
                            stem_str,
                        );
                    }
                }
            }
        }
    }
    files.sort();
    files
}


/// Check if a string is a valid Rust identifier (only `[a-z0-9_]`, must not start with a digit).
pub(crate) fn is_valid_rust_ident(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }
    let first = s.as_bytes()[0];
    if first.is_ascii_digit() {
        return false;
    }
    s.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}


/// Scan a `.rs` file to determine which custom lint functions it defines.
///
/// Looks for `pub fn lint()` and `pub fn cross_lint()` signatures.
pub(crate) fn scan_lint_functions(lints_dir: &Path, stem: &str) -> (bool, bool) {
    let path = lints_dir.join(format!("{stem}.rs"));
    let content = fs::read_to_string(&path).unwrap_or_default();

    let has_lint = content.contains("pub fn lint(");
    let has_cross_lint = content.contains("pub fn cross_lint(");

    (has_lint, has_cross_lint)
}


/// Generate the proxy's main.rs with custom lint module includes.
///
/// In-tree lint files: each `.rs` file under `{mock_dir}/lints/` is included
/// via `#[path]` attribute. Each file must define:
/// - `pub fn lint() -> Box<dyn mockspace_lint_rules::Lint>` for per-crate lints
/// - `pub fn cross_lint() -> Box<dyn mockspace_lint_rules::CrossCrateLint>` for cross-crate lints
///
/// External lint packs: each crate named in `[lint-crates]` is pulled in as
/// a normal cargo dependency. Each pack must expose:
/// - `pub fn lints() -> Vec<Box<dyn mockspace_lint_rules::Lint>>`
/// - `pub fn cross_lints() -> Vec<Box<dyn mockspace_lint_rules::CrossCrateLint>>`
pub(crate) fn generate_custom_lint_main(
    lint_files: &[String],
    lints_dir: &Path,
    lint_packs: &[(String, String)],
) -> String {
    let mut out = String::new();

    // Module declarations with absolute paths (forward slashes for cross-platform compat)
    for name in lint_files {
        let abs_path = lints_dir.join(format!("{name}.rs"));
        let path_str = abs_path.display().to_string().replace('\\', "/");
        out.push_str(&format!(
            "#[path = \"{path_str}\"]\nmod {name};\n",
        ));
    }
    out.push('\n');

    // Scan each file to determine which functions it provides
    let mut lint_mods = Vec::new();
    let mut cross_lint_mods = Vec::new();

    for name in lint_files {
        let (has_lint, has_cross_lint) = scan_lint_functions(lints_dir, name);
        if has_lint {
            lint_mods.push(name.as_str());
        }
        if has_cross_lint {
            cross_lint_mods.push(name.as_str());
        }
    }

    // Cargo names with `-` become `_` for Rust paths.
    let pack_idents: Vec<String> = lint_packs
        .iter()
        .map(|(name, _)| name.replace('-', "_"))
        .collect();

    // custom_lints() function
    out.push_str("fn custom_lints() -> Vec<Box<dyn mockspace::Lint>> {\n");
    out.push_str("    let mut v: Vec<Box<dyn mockspace::Lint>> = Vec::new();\n");
    for name in &lint_mods {
        out.push_str(&format!("    v.push({name}::lint());\n"));
    }
    for ident in &pack_idents {
        out.push_str(&format!("    v.extend({ident}::lints());\n"));
    }
    out.push_str("    v\n");
    out.push_str("}\n\n");

    // custom_cross_lints() function
    out.push_str("fn custom_cross_lints() -> Vec<Box<dyn mockspace::CrossCrateLint>> {\n");
    out.push_str("    let mut v: Vec<Box<dyn mockspace::CrossCrateLint>> = Vec::new();\n");
    for name in &cross_lint_mods {
        out.push_str(&format!("    v.push({name}::cross_lint());\n"));
    }
    for ident in &pack_idents {
        out.push_str(&format!("    v.extend({ident}::cross_lints());\n"));
    }
    out.push_str("    v\n");
    out.push_str("}\n\n");

    out.push_str("fn main() -> std::process::ExitCode {\n");
    out.push_str("    mockspace::run_with_custom_lints(custom_lints(), custom_cross_lints())\n");
    out.push_str("}\n");

    out
}

// ──────────────────────────────────────────────────────────────────────
// Generated hooks (core.hooksPath target)
// ──────────────────────────────────────────────────────────────────────

