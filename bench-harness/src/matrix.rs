//! Declarative axis-matrix and variant-crate generation.
//!
//! A composition bench sweeps several axes (dispatch shape, record layout, value
//! representation, program profile, ...) and needs one variant cdylib crate per
//! point in the cartesian product, each wrapping the right library function with
//! `#[bench_variant]`, plus a `bench.toml` section wiring the variants together.
//! Consumers used to generate all of that with bespoke Python scripts, one per
//! bench, each re-encoding the same cartesian-product-then-scaffold logic around
//! a slightly different `lib.rs` body.
//!
//! This module makes the identical part first-class: it reads a declarative
//! [`MatrixSpec`] (axes and their tagged values), expands the cartesian product,
//! and for each composition emits a variant crate (a `Cargo.toml` with the
//! carrier dependency plus the union of the axis-selected features, and a
//! `src/lib.rs` rendered from the consumer's template with the composition's
//! substitutions) and a `bench.toml` section (title, baseline, sizes, variant
//! paths). What stays a consumer template is the `lib.rs` body, because that is
//! the only genuinely bench-specific part: which functions it calls and how it
//! folds the output. The axis expansion, feature selection, crate scaffold, and
//! section wiring, the parts every generator duplicated, are the harness's.
//!
//! Substitution is deliberately simple: `{key}` in the templates is replaced by
//! the value the composition binds for `key`. A composition binds one key per
//! axis (the axis name to the chosen value's `subst`, or its `tag` if no explicit
//! subst), plus `name` (the generated variant name) and `n` is left for the
//! bench framework's size dispatch. No expression language: a template that needs
//! logic is a sign the axis set is wrong, not that the substituter should grow.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

/// One value along an axis. `tag` names it in the variant name; `subst` provides
/// the template substitutions this value contributes (e.g. the function name to
/// call); `features` are carrier cargo features this value requires.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct AxisValue {
    pub tag:      String,
    #[serde(default)]
    pub subst:    BTreeMap<String, String>,
    #[serde(default)]
    pub features: Vec<String>,
}

/// One axis: a name and its values. The cartesian product is taken over all
/// axes' values in declaration order.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct AxisSpec {
    pub name:   String,
    pub values: Vec<AxisValue>,
}

/// A full matrix specification for one composition bench.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct MatrixSpec {
    /// Bench name; the generated `bench.toml` section is `[bench.<bench>]`.
    pub bench:             String,
    /// Report title; `{key}` substituted per composition (so the title can name
    /// the axis values, though most benches keep it fixed).
    pub title:             String,
    /// The dependency line body for the carrier crate. This IS rendered, so its
    /// literal TOML inline-table braces must be escaped as `{{` / `}}`, e.g.
    /// `vehje-bench-carrier = {{ path = "../../carrier"{carrier_features} }}`.
    /// `{carrier_features}` is replaced by `, features = [..]` (or empty).
    pub carrier_dep:       String,
    /// Extra dependency lines emitted verbatim (NOT rendered, so their braces
    /// need no escaping) into every variant `Cargo.toml` (bench-core, bench-macro
    /// git deps).
    #[serde(default)]
    pub extra_deps:        Vec<String>,
    pub master_seed:       String,
    pub sizes:             Vec<usize>,
    /// The generated variant whose name contains this substring is the analysis
    /// baseline. If none matches, the first variant is the baseline.
    #[serde(default)]
    pub baseline_contains: Option<String>,
    /// `subtract` (default), `ratio`, or `percent`; emitted into the section's
    /// `[bench.<bench>.normalise]` block.
    #[serde(default)]
    pub normalise_mode:    Option<String>,
    /// The variant-name template. `{axis}` for each axis binds to that axis
    /// value's tag; a common form is `carrier_{shape}_{layout}`.
    pub name_template:      String,
    /// The `src/lib.rs` body template, rendered per composition.
    pub lib_template:       String,
    pub axes:               Vec<AxisSpec>,
}

/// Keys the generator injects itself; an axis name or a value `subst` key that
/// collides with one of these would be silently overwritten (or overwrite it),
/// so `validate` rejects the spec instead. `n` is left for the bench framework's
/// per-size dispatch, so a template's `{n}` must survive to the generated crate.
const RESERVED_KEYS: &[&str] = &["name", "carrier_features", "n"];

impl MatrixSpec {
    /// Reject a spec that would expand to nonsense: no axes (the product is one
    /// empty composition, leaving `{axis}` braces in the rendered name), an axis
    /// with no values (the product collapses to zero variants), or a name that
    /// collides with a generator-injected key. Callers that expand or generate a
    /// spec should validate it first; `generate` does so before writing anything.
    pub fn validate(&self) -> Result<(), String> {
        if self.axes.is_empty() {
            return Err("matrix has no axes; the product is a single empty composition".into());
        }
        for axis in &self.axes {
            if axis.values.is_empty() {
                return Err(format!("axis `{}` has no values; the product collapses to zero variants", axis.name));
            }
            if RESERVED_KEYS.contains(&axis.name.as_str()) {
                return Err(format!("axis name `{}` collides with a generator-injected key", axis.name));
            }
            for val in &axis.values {
                for key in val.subst.keys() {
                    if RESERVED_KEYS.contains(&key.as_str()) {
                        return Err(format!("value `{}` subst key `{}` collides with a generator-injected key", val.tag, key));
                    }
                    if self.axes.iter().any(|a| &a.name == key) {
                        return Err(format!("value `{}` subst key `{}` collides with axis name `{}`", val.tag, key, key));
                    }
                }
            }
        }
        Ok(())
    }
}

/// One expanded point in the matrix: the chosen value per axis, the resolved
/// variant name, the union of required features, and the substitution map.
#[derive(Clone, Debug, PartialEq)]
pub struct Composition {
    pub name:     String,
    pub features: Vec<String>,
    pub subst:    BTreeMap<String, String>,
}

/// Expand the cartesian product of the axes. Each composition binds every axis
/// name to its chosen value (via `subst`, defaulting to `{axis} -> tag`), unions
/// the selected features, and renders the variant name from `name_template`.
pub fn expand(spec: &MatrixSpec) -> Vec<Composition> {
    let mut acc: Vec<Composition> = vec![Composition {
        name: String::new(),
        features: Vec::new(),
        subst: BTreeMap::new(),
    }];
    for axis in &spec.axes {
        let mut next = Vec::with_capacity(acc.len() * axis.values.len().max(1));
        for base in &acc {
            for val in &axis.values {
                let mut c = base.clone();
                // The axis name always binds to the value's tag (used by the
                // variant name), and the value's own subst keys (distinct from
                // axis names by convention: fn_name, a numeric vocab, ...) are
                // merged for the lib-body template.
                c.subst.insert(axis.name.clone(), val.tag.clone());
                for (k, v) in &val.subst {
                    c.subst.insert(k.clone(), v.clone());
                }
                for f in &val.features {
                    if !c.features.contains(f) {
                        c.features.push(f.clone());
                    }
                }
                next.push(c);
            }
        }
        acc = next;
    }
    // Resolve names once all axis tags are bound. name_template uses the axis
    // tags directly (the tag, not the subst), so bind tag-keyed values too.
    for c in &mut acc {
        c.features.sort();
        c.name = render(&spec.name_template, &c.subst);
    }
    acc
}

/// Replace every `{key}` in `template` with `subst[key]`. Unknown keys are left
/// verbatim (so a template can carry literal `{n}` for the framework's size
/// dispatch). A `{{` / `}}` escapes a literal brace.
pub fn render(template: &str, subst: &BTreeMap<String, String>) -> String {
    let mut out = String::with_capacity(template.len());
    let bytes = template.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'{' && i + 1 < bytes.len() && bytes[i + 1] == b'{' {
            out.push('{');
            i += 2;
        } else if bytes[i] == b'}' && i + 1 < bytes.len() && bytes[i + 1] == b'}' {
            out.push('}');
            i += 2;
        } else if bytes[i] == b'{' {
            if let Some(close) = template[i + 1 ..].find('}') {
                let key = &template[i + 1 .. i + 1 + close];
                match subst.get(key) {
                    Some(v) => out.push_str(v),
                    None => {
                        // leave unknown placeholder verbatim
                        out.push('{');
                        out.push_str(key);
                        out.push('}');
                    }
                }
                i += 1 + close + 1;
            } else {
                out.push('{');
                i += 1;
            }
        } else {
            // copy the whole UTF-8 char; `bytes[i] as char` corrupts multibyte.
            // the brace branches above stay byte-indexed safely: `{`/`}` are
            // ASCII, and a multibyte char's bytes are all >= 0x80, so they never
            // match those branches.
            let ch = template[i ..].chars().next().unwrap();
            out.push(ch);
            i += ch.len_utf8();
        }
    }
    out
}

/// The rendered artefacts for one composition: the crate files to write and the
/// path the bench section should reference.
pub struct GeneratedVariant {
    pub name:        String,
    pub cargo_toml:  String,
    pub lib_rs:      String,
    /// Relative path the bench.toml section references (the built cdylib).
    pub bench_path:  String,
}

/// Render (but do not write) the variant crate for one composition.
pub fn render_variant(spec: &MatrixSpec, c: &Composition) -> GeneratedVariant {
    let carrier_features = if c.features.is_empty() {
        String::new()
    } else {
        let list = c.features.iter().map(|f| format!("\"{f}\"")).collect::<Vec<_>>().join(", ");
        format!(", features = [{list}]")
    };
    let mut subst = c.subst.clone();
    subst.insert("carrier_features".to_string(), carrier_features);
    subst.insert("name".to_string(), c.name.clone());

    let carrier_dep = render(&spec.carrier_dep, &subst);
    let mut cargo = String::new();
    cargo.push_str("[workspace]\n[package]\n");
    cargo.push_str(&format!("name = \"{}\"\nversion = \"0.0.0\"\nedition = \"2021\"\npublish = false\n", c.name));
    cargo.push_str(&format!("[lib]\nname = \"{}\"\npath = \"src/lib.rs\"\ncrate-type = [\"cdylib\"]\n", c.name));
    cargo.push_str("[dependencies]\n");
    for d in &spec.extra_deps {
        cargo.push_str(d);
        cargo.push('\n');
    }
    cargo.push_str(&carrier_dep);
    cargo.push('\n');
    cargo.push_str("[profile.release]\nopt-level = 3\nlto = \"fat\"\ncodegen-units = 1\n");

    let lib_rs = render(&spec.lib_template, &subst);
    GeneratedVariant {
        name: c.name.clone(),
        cargo_toml: cargo,
        lib_rs,
        bench_path: format!("variants/{0}/target/release/{0}", c.name),
    }
}

/// Render the `bench.toml` section (title, seed, normalise, per-size variant
/// lists) for the whole matrix.
pub fn render_bench_section(spec: &MatrixSpec, comps: &[Composition]) -> String {
    let names: Vec<&str> = comps.iter().map(|c| c.name.as_str()).collect();
    let baseline = spec
        .baseline_contains
        .as_deref()
        .and_then(|needle| names.iter().find(|n| n.contains(needle)).copied())
        .unwrap_or_else(|| names.first().copied().unwrap_or(""));
    let title_subst = BTreeMap::new();
    let mut out = String::new();
    out.push_str(&format!("[bench.{}]\n", spec.bench));
    out.push_str(&format!("title = \"{}\"\n", render(&spec.title, &title_subst)));
    out.push_str("workload = \"realistic\"\n");
    out.push_str(&format!("master_seed = {}\n", spec.master_seed));
    out.push_str(&format!("[bench.{}.normalise]\n", spec.bench));
    out.push_str(&format!("baseline = \"{baseline}\"\n"));
    out.push_str(&format!("mode = \"{}\"\n", spec.normalise_mode.as_deref().unwrap_or("subtract")));
    let paths = comps
        .iter()
        .map(|c| format!("\"variants/{0}/target/release/{0}\"", c.name))
        .collect::<Vec<_>>()
        .join(", ");
    for n in &spec.sizes {
        out.push_str(&format!("[[bench.{}.sizes]]\n", spec.bench));
        out.push_str(&format!("n = {n}\n"));
        out.push_str(&format!("variants = [{paths}]\n"));
    }
    out
}

/// Generate every variant crate under `out_dir/variants/<name>/` and return the
/// `bench.toml` section. Writes `Cargo.toml` and `src/lib.rs` per variant.
pub fn generate(spec: &MatrixSpec, out_dir: &Path) -> std::io::Result<String> {
    spec.validate().map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?;
    let comps = expand(spec);
    for c in &comps {
        let v = render_variant(spec, c);
        let crate_dir = out_dir.join("variants").join(&v.name);
        std::fs::create_dir_all(crate_dir.join("src"))?;
        std::fs::write(crate_dir.join("Cargo.toml"), v.cargo_toml)?;
        std::fs::write(crate_dir.join("src").join("lib.rs"), v.lib_rs)?;
    }
    Ok(render_bench_section(spec, &comps))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dispatch_spec() -> MatrixSpec {
        let av = |tag: &str, fnname: &str, feats: &[&str]| AxisValue {
            tag: tag.to_string(),
            subst: BTreeMap::from([("fn_name".to_string(), fnname.to_string())]),
            features: feats.iter().map(|s| s.to_string()).collect(),
        };
        let lv = |tag: &str, val: &str| AxisValue {
            tag: tag.to_string(),
            subst: BTreeMap::from([("vocab_num".to_string(), val.to_string())]),
            features: vec![],
        };
        MatrixSpec {
            bench: "carrier_dispatch".into(),
            title: "Dispatch shape sweep".into(),
            carrier_dep: "vehje-bench-carrier = {{ path = \"../../carrier\"{carrier_features} }}".into(),
            extra_deps: vec!["mockspace-bench-core = { path = \"x\" }".into()],
            master_seed: "0x5eed".into(),
            sizes: vec![64, 256],
            baseline_contains: Some("switch".into()),
            normalise_mode: None,
            name_template: "carrier_{shape}_{vocab}".into(),
            lib_template: "// {shape} over {vocab_num}; calls {fn_name}; name {name}".into(),
            axes: vec![
                AxisSpec {
                    name: "vocab".into(),
                    values: vec![lv("v4", "4"), lv("v17", "17")],
                },
                AxisSpec {
                    name: "shape".into(),
                    values: vec![
                        av("switch", "interpret", &[]),
                        av("fntable", "interpret_fntable", &[]),
                        av("threaded", "interpret_threaded", &["threaded"]),
                    ],
                },
            ],
        }
    }

    #[test]
    fn cartesian_product_is_complete() {
        let comps = expand(&dispatch_spec());
        assert_eq!(comps.len(), 6, "2 vocab x 3 shape = 6 compositions");
        let names: Vec<&str> = comps.iter().map(|c| c.name.as_str()).collect();
        assert!(names.contains(&"carrier_switch_v4"));
        assert!(names.contains(&"carrier_threaded_v17"));
    }

    #[test]
    fn features_union_per_composition() {
        let comps = expand(&dispatch_spec());
        for c in &comps {
            if c.name.contains("threaded") {
                assert_eq!(c.features, vec!["threaded".to_string()], "{}", c.name);
            } else {
                assert!(c.features.is_empty(), "{} should need no features", c.name);
            }
        }
    }

    #[test]
    fn lib_template_renders_substitutions() {
        let comps = expand(&dispatch_spec());
        let spec = dispatch_spec();
        let threaded_v17 = comps.iter().find(|c| c.name == "carrier_threaded_v17").unwrap();
        let v = render_variant(&spec, threaded_v17);
        assert!(v.lib_rs.contains("interpret_threaded"), "fn_name substituted");
        assert!(v.lib_rs.contains("threaded over 17"), "shape+vocab_num substituted");
        assert!(v.cargo_toml.contains("features = [\"threaded\"]"), "carrier features wired");
        let switch_v4 = comps.iter().find(|c| c.name == "carrier_switch_v4").unwrap();
        let vs = render_variant(&spec, switch_v4);
        assert!(!vs.cargo_toml.contains("features ="), "no features for switch");
    }

    #[test]
    fn bench_section_names_baseline_and_sizes() {
        let spec = dispatch_spec();
        let comps = expand(&spec);
        let section = render_bench_section(&spec, &comps);
        assert!(section.contains("[bench.carrier_dispatch]"));
        assert!(section.contains("baseline = \"carrier_switch_v4\""), "switch is baseline");
        assert!(section.contains("mode = \"subtract\""));
        assert!(section.contains("n = 64") && section.contains("n = 256"));
    }

    #[test]
    fn render_escapes_and_unknown_keys() {
        let s = BTreeMap::from([("x".to_string(), "X".to_string())]);
        assert_eq!(render("a{x}b", &s), "aXb");
        assert_eq!(render("{{lit}}", &s), "{lit}");
        assert_eq!(render("{unknown}", &s), "{unknown}", "unknown left verbatim");
    }

    #[test]
    fn render_preserves_multibyte() {
        // regression: `bytes[i] as char` corrupted non-ASCII literal text. A
        // template carrying `x base` in its title, an em-dash-free arrow, or any
        // UTF-8 must round-trip, including a multibyte char adjacent to a key.
        let s = BTreeMap::from([("v".to_string(), "V".to_string())]);
        assert_eq!(render("ratio x\u{00d7} base", &s), "ratio x\u{00d7} base");
        assert_eq!(render("\u{00d7}{v}\u{00d7}", &s), "\u{00d7}V\u{00d7}");
        assert_eq!(render("caf\u{00e9} {v} r\u{00e9}sum\u{00e9}", &s), "caf\u{00e9} V r\u{00e9}sum\u{00e9}");
    }

    #[test]
    fn validate_accepts_a_well_formed_spec() {
        assert!(dispatch_spec().validate().is_ok());
    }

    #[test]
    fn validate_rejects_empty_axes() {
        let mut spec = dispatch_spec();
        spec.axes.clear();
        assert!(spec.validate().unwrap_err().contains("no axes"));
    }

    #[test]
    fn validate_rejects_axis_with_no_values() {
        let mut spec = dispatch_spec();
        spec.axes[0].values.clear();
        let e = spec.validate().unwrap_err();
        assert!(e.contains("no values") && e.contains("vocab"), "{e}");
    }

    #[test]
    fn validate_rejects_reserved_axis_name() {
        let mut spec = dispatch_spec();
        spec.axes[0].name = "name".into();
        assert!(spec.validate().unwrap_err().contains("collides"));
    }

    #[test]
    fn validate_rejects_reserved_subst_key() {
        let mut spec = dispatch_spec();
        spec.axes[0].values[0].subst.insert("carrier_features".into(), "oops".into());
        assert!(spec.validate().unwrap_err().contains("carrier_features"));
    }

    #[test]
    fn validate_rejects_subst_key_colliding_with_axis_name() {
        let mut spec = dispatch_spec();
        // a value on `shape` binds a subst key named `vocab`, an existing axis.
        spec.axes[1].values[0].subst.insert("vocab".into(), "clash".into());
        let e = spec.validate().unwrap_err();
        assert!(e.contains("axis name") && e.contains("vocab"), "{e}");
    }

    #[test]
    fn generate_errors_on_invalid_spec() {
        let mut spec = dispatch_spec();
        spec.axes[1].values.clear();
        let dir = std::env::temp_dir().join(format!("mx_bad_{}", std::process::id()));
        let err = generate(&spec, &dir).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
        assert!(!dir.join("variants").exists(), "nothing written for an invalid spec");
    }

    #[test]
    fn generate_writes_crates() {
        let spec = dispatch_spec();
        let dir = std::env::temp_dir().join(format!("mx_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let section = generate(&spec, &dir).expect("generate");
        assert!(section.contains("[bench.carrier_dispatch]"));
        let libp = dir.join("variants/carrier_threaded_v17/src/lib.rs");
        assert!(libp.exists(), "variant lib.rs written");
        let lib = std::fs::read_to_string(libp).unwrap();
        assert!(lib.contains("interpret_threaded"));
        let cargo = std::fs::read_to_string(dir.join("variants/carrier_threaded_v17/Cargo.toml")).unwrap();
        assert!(cargo.contains("crate-type = [\"cdylib\"]"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
