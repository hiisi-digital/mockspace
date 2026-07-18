use super::*;

/// Strip the placeholder delimiters from a line, yielding each expression.
///
/// References in prose are written `{{ reg::vocab::xpbd }}`, in the same
/// placeholder vocabulary the rest of the template system uses. The braces are
/// not ceremony: they make a reference something the author states rather than
/// something the renderer guesses at from a pattern. Without them a project
/// with a root named `core` would silently link `core::mem::12`, and prose
/// about code would be rewritten by accident.
///
/// One syntax then covers everything: `{{ reg::ns::slug }}` for a row,
/// `{{ reg::ns::slug::field }}` for a field, `{{ reg::ns }}` for a whole
/// table, and `{{ root::path::line }}` for a file citation.
///
/// Provenance fields in the data need no braces: the field is already declared
/// to hold references, so there is nothing to disambiguate.
pub fn placeholder_exprs(line: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut rest = line;
    while let Some(s) = rest.find("{{") {
        let after = &rest[s + 2..];
        let Some(e) = after.find("}}") else { break };
        let inner = after[..e].trim().to_string();
        if !inner.is_empty() {
            out.push((format!("{{{{{}}}}}", &after[..e]), inner));
        }
        rest = &after[e + 2..];
    }
    out
}

/// Every `reg::` reference in `text`, as `(qualified, field)` pairs.
///
/// Explicit rather than scanned. An earlier form auto-linked any token that
/// looked like an identifier, which meant guessing whether a token was a
/// reference at all. Requiring `reg::` makes a reference something the author
/// states.
pub fn find_registry_refs(text: &str) -> Vec<(String, Option<String>)> {
    let prefix = format!("{REGISTRY_ROOT}::");
    let mut found = Vec::new();
    let mut in_fence = false;
    for line in text.lines() {
        if line.trim_start().starts_with("```") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            continue;
        }
        // Only inside a placeholder. A bare `reg::ns::slug` in prose is not a
        // reference the renderer touches, so reporting it as dangling would
        // flag documentation that merely describes the syntax. The document
        // defining this syntax was the first thing to trip it.
        for (_, expr) in placeholder_exprs(line) {
            let line = expr.as_str();
        let mut rest = line;
        while let Some(pos) = rest.find(&prefix) {
            let before_ok = pos == 0
                || !rest[..pos]
                    .chars()
                    .next_back()
                    .map(|c| c.is_alphanumeric() || c == '_' || c == ':')
                    .unwrap_or(false);
            let after = &rest[pos + prefix.len()..];
            if before_ok {
                let seg = |s: &str| -> String {
                    s.chars().take_while(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == '_').collect()
                };
                let ns = seg(after);
                if !ns.is_empty() {
                    let t1 = &after[ns.len()..];
                    if let Some(t1) = t1.strip_prefix("::") {
                        let slug = seg(t1);
                        if !slug.is_empty() {
                            let t2 = &t1[slug.len()..];
                            let field = t2.strip_prefix("::").map(seg).filter(|f| !f.is_empty());
                            found.push((format!("{ns}::{slug}"), field));
                        }
                    }
                }
            }
            rest = &rest[pos + prefix.len()..];
        }
        }
    }
    found
}

/// References naming a row that does not exist, or a field that row does not
/// carry. Both are silent without a check: the first renders as nothing
/// useful, the second renders as empty.
pub fn dangling_references(text: &str, reg: &Registry) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for (qualified, field) in find_registry_refs(text) {
        match reg.get(&qualified) {
            None => {
                out.insert(format!("{REGISTRY_ROOT}::{qualified}"));
            }
            Some(row) => {
                if let Some(f) = &field {
                    if f != "id" && !row.fields.contains_key(f) {
                        out.insert(format!("{REGISTRY_ROOT}::{qualified}::{f}"));
                    }
                }
            }
        }
    }
    out
}

/// What a citation points at within a file.
///
/// Line numbers fail SILENTLY. An edit anywhere above a cited line shifts it,
/// the citation still resolves, and it now points at different content. That is
/// the worst failure shape this project recognises: the check passes and the
/// answer is wrong. Only "past the end of the file" fails loudly, and that is
/// the case that matters least.
///
/// A heading fails loudly instead. Rename it and the citation stops resolving,
/// which is a report rather than a lie. Headings are therefore the default, and
/// line numbers belong to roots that do not move.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Anchor {
    /// A heading slug: `#the-four-lanes`. Survives every edit that does not
    /// rename the heading, and announces itself when one does.
    Heading(String),
    /// An explicit line. Honest only where the file does not change, which is
    /// why a root carrying line citations should declare itself frozen.
    Line(usize),
}

/// A file citation: `root::seg::seg...::anchor`.
///
/// The last segment is the line and everything between the root and it joins
/// as a path, so one rule covers `mock::DESIGN::12` and
/// `mock::crates::numeric::DESIGN::12` without either needing its own root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileRef {
    pub root: String,
    pub path: String,
    pub anchor: Anchor,
}

impl FileRef {
    pub fn parse(s: &str) -> Option<Self> {
        let parts: Vec<&str> = s.split("::").collect();
        if parts.len() < 3 {
            return None;
        }
        let last = parts.last()?.trim();
        let anchor = if let Some(h) = last.strip_prefix('#') {
            if h.is_empty() {
                return None;
            }
            Anchor::Heading(h.to_string())
        } else {
            let n = last.parse::<usize>().ok()?;
            if n == 0 {
                return None;
            }
            Anchor::Line(n)
        };
        let root = parts[0].trim();
        if root.is_empty() {
            return None;
        }
        let path = parts[1..parts.len() - 1].join("/");
        if path.is_empty() {
            return None;
        }
        Some(Self { root: root.to_string(), path, anchor })
    }

    pub fn render(&self) -> String {
        let a = match &self.anchor {
            Anchor::Heading(h) => format!("#{h}"),
            Anchor::Line(n) => n.to_string(),
        };
        format!("{}::{}::{}", self.root, self.path.replace('/', "::"), a)
    }
}

/// Resolve a citation's path under a root, allowing the extension to be
/// omitted.
///
/// `mock::DESIGN::12` finds `DESIGN.md.tmpl` without the author having to know
/// the extension. That is the real motivation: the same document exists as a
/// template in one root and as rendered output in another, and a citation
/// should not have to track which.
///
/// Exactly one match resolves. Several is an error rather than a guess: the
/// author meant one of them, and picking silently would point the citation
/// somewhere they did not choose.
pub enum PathResolution {
    Found(PathBuf),
    Missing,
    Ambiguous(Vec<String>),
}

pub fn resolve_cited_path(root_dir: &Path, path: &str) -> PathResolution {
    let exact = root_dir.join(path);
    if exact.is_file() {
        return PathResolution::Found(exact);
    }
    let target = Path::new(path);
    let dir = match target.parent() {
        Some(p) if !p.as_os_str().is_empty() => root_dir.join(p),
        _ => root_dir.to_path_buf(),
    };
    let Some(stem) = target.file_name().map(|s| s.to_string_lossy().to_string()) else {
        return PathResolution::Missing;
    };
    let Ok(entries) = fs::read_dir(&dir) else {
        return PathResolution::Missing;
    };
    let mut matches: Vec<PathBuf> = entries
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            p.is_file()
                && p.file_name()
                    .map(|n| {
                        let n = n.to_string_lossy();
                        n == stem || n.starts_with(&format!("{stem}."))
                    })
                    .unwrap_or(false)
        })
        .collect();
    matches.sort();
    match matches.len() {
        0 => PathResolution::Missing,
        1 => PathResolution::Found(matches.remove(0)),
        _ => PathResolution::Ambiguous(
            matches
                .iter()
                .filter_map(|p| p.file_name().map(|n| n.to_string_lossy().to_string()))
                .collect(),
        ),
    }
}

/// A relative path from `from` (a directory) to `to`, in forge-link form.
pub(crate) fn relative_from(from: &Path, to: &Path) -> String {
    let f: Vec<_> = from.components().collect();
    let t: Vec<_> = to.components().collect();
    let common = f.iter().zip(t.iter()).take_while(|(a, b)| a == b).count();
    let mut parts: Vec<String> = vec!["..".to_string(); f.len() - common];
    parts.extend(t[common..].iter().map(|c| c.as_os_str().to_string_lossy().to_string()));
    if parts.is_empty() { ".".to_string() } else { parts.join("/") }
}

/// Rewrite file citations in a rendered document into line links.
pub fn resolve_doc_refs(
    text: &str,
    roots: &BTreeMap<String, String>,
    repo_root: &Path,
    docs_dir: &Path,
) -> String {
    if roots.is_empty() || !text.contains("::") {
        return text.to_string();
    }
    let mut out = String::with_capacity(text.len());
    let mut in_fence = false;
    for (i, line) in text.lines().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        if line.trim_start().starts_with("```") {
            in_fence = !in_fence;
            out.push_str(line);
            continue;
        }
        if in_fence {
            out.push_str(line);
            continue;
        }
        let mut rewritten = line.to_string();
        for tok in citation_tokens(line) {
            let Some(r) = FileRef::parse(&tok) else { continue };
            if r.root == REGISTRY_ROOT {
                continue;
            }
            let Some(rel) = roots.get(&r.root) else { continue };
            if let PathResolution::Found(target) =
                resolve_cited_path(&repo_root.join(rel), &r.path)
            {
                let link = relative_from(docs_dir, &target);
                rewritten = rewritten.replace(
                    &tok,
                    &match &r.anchor {
                        Anchor::Heading(h) => {
                            format!("[{}/{}#{h}]({}#{h})", r.root, r.path, link)
                        }
                        Anchor::Line(n) => {
                            format!("[{}/{}:{n}]({}#L{n})", r.root, r.path, link)
                        }
                    },
                );
            }
        }
        out.push_str(&rewritten);
    }
    if text.ends_with('\n') {
        out.push('\n');
    }
    out
}

/// Citation-shaped tokens in a line: a run of `::`-joined segments ending in
/// digits. Bounded by whitespace and the characters that bracket a citation in
/// prose, so a reference inside parentheses or backticks does not swallow them.
fn citation_tokens(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    for raw in line.split(|c: char| c.is_whitespace() || "()[]`\"',;".contains(c)) {
        if raw.matches("::").count() >= 2 && raw.split("::").last().map(|s| !s.is_empty() && s.chars().all(|c| c.is_ascii_digit())).unwrap_or(false) {
            out.push(raw.to_string());
        }
    }
    out
}

/// Resolve an anchor to a line in `path`.
///
/// A heading matches on its slug: lowercased, non-alphanumerics collapsed to
/// hyphens, which is what forges generate and therefore what a reader can also
/// click to.
pub fn resolve_anchor(path: &Path, anchor: &Anchor) -> Option<usize> {
    match anchor {
        Anchor::Line(n) => {
            let count = fs::read_to_string(path).ok()?.lines().count();
            if *n <= count { Some(*n) } else { None }
        }
        Anchor::Heading(want) => {
            let text = fs::read_to_string(path).ok()?;
            text.lines().enumerate().find_map(|(i, l)| {
                let trimmed = l.trim_start();
                if !trimmed.starts_with('#') {
                    return None;
                }
                let title = trimmed.trim_start_matches('#').trim();
                (heading_slug(title) == *want).then_some(i + 1)
            })
        }
    }
}

/// The forge-compatible slug for a heading.
pub fn heading_slug(title: &str) -> String {
    let mut out = String::with_capacity(title.len());
    let mut last_dash = false;
    for c in title.chars() {
        if c.is_alphanumeric() {
            for l in c.to_lowercase() {
                out.push(l);
            }
            last_dash = false;
        } else if !last_dash && !out.is_empty() {
            out.push('-');
            last_dash = true;
        }
    }
    out.trim_end_matches('-').to_string()
}
