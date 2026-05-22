//! Builtin markdown fragments injected into rendered docs by the
//! render pipeline.
//!
//! These exist because the canonical AI/agent responsibility notice
//! (#245) and the canonical workflow description (#246) are
//! mockspace-internal content that every consumer should ship
//! identically. Authoring each by hand in every consumer's
//! `mock/*.md.tmpl` invites drift; centralising the canonical text
//! here and auto-injecting it at render time keeps every consumer
//! aligned on the same wording without the per-repo authoring tax.
//!
//! Each fragment is embedded via `include_str!` so updates land in
//! consumer rendered output as soon as the consumer rebuilds the
//! mockspace binary and re-runs `cargo mock regenerate`.
//!
//! Idempotency contract: each fragment carries a `<!-- ... -->` HTML
//! comment marker on its first line. The render pipeline checks for
//! that marker in the rendered output before injecting; templates
//! that author the notice by hand can include the same marker to
//! suppress auto-injection.

/// The full AI/agent responsibility notice for WORKFLOW.md (Form A
/// per `ai-agent-framing.md`). Prose form. Carries the
/// `<!-- mockspace:ai-notice-form-a -->` marker.
pub const AI_NOTICE_FORM_A: &str = include_str!("ai_notice_form_a.md");

/// The short rule-list form of the AI responsibility notice for
/// PRINCIPLES.md (Form B per `ai-agent-framing.md`). Carries the
/// `<!-- mockspace:ai-notice-form-b -->` marker.
pub const AI_NOTICE_FORM_B: &str = include_str!("ai_notice_form_b.md");

/// Marker comment present in [`AI_NOTICE_FORM_A`]. The render pipeline
/// scans rendered output for this string before injecting Form A;
/// presence suppresses injection so a template that authors the
/// notice itself (perhaps with project-specific paragraphs) is not
/// double-stamped.
pub const AI_NOTICE_FORM_A_MARKER: &str = "<!-- mockspace:ai-notice-form-a -->";

/// Marker comment present in [`AI_NOTICE_FORM_B`]. Same role as
/// [`AI_NOTICE_FORM_A_MARKER`] but for the PRINCIPLES.md form.
pub const AI_NOTICE_FORM_B_MARKER: &str = "<!-- mockspace:ai-notice-form-b -->";

/// Append `fragment` to `rendered` when `marker` is not already
/// present. The marker presence check is a single `str::contains`;
/// the fragment itself carries the marker on its first line so a
/// later regenerate pass sees the marker and skips re-injection.
///
/// Returns the (possibly-extended) rendered string. The render
/// pipeline calls this between the template-render step and the
/// atomic-write step.
pub fn inject_if_absent(rendered: String, marker: &str, fragment: &str) -> String {
    if rendered.contains(marker) {
        return rendered;
    }
    let mut out = rendered;
    if !out.ends_with('\n') {
        out.push('\n');
    }
    out.push('\n');
    out.push_str(fragment);
    if !out.ends_with('\n') {
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inject_appends_when_marker_absent() {
        let body = "preamble\n".to_string();
        let out = inject_if_absent(body, "<!-- m -->", "<!-- m -->\nbody\n");
        assert!(out.contains("preamble"));
        assert!(out.contains("<!-- m -->"));
        assert!(out.contains("body"));
    }

    #[test]
    fn inject_skips_when_marker_present() {
        let body = "preamble with <!-- m --> in it\n".to_string();
        let out = inject_if_absent(body.clone(), "<!-- m -->", "ignored");
        assert_eq!(out, body);
    }

    #[test]
    fn inject_handles_missing_trailing_newline() {
        let body = "no-newline-here".to_string();
        let out = inject_if_absent(body, "<!-- m -->", "<!-- m -->\nfragment");
        assert!(out.starts_with("no-newline-here\n"));
        assert!(out.contains("fragment"));
        assert!(out.ends_with('\n'));
    }

    #[test]
    fn form_a_const_carries_its_marker() {
        assert!(
            AI_NOTICE_FORM_A.contains(AI_NOTICE_FORM_A_MARKER),
            "Form A fragment must carry its marker so auto-injection is idempotent"
        );
    }

    #[test]
    fn form_b_const_carries_its_marker() {
        assert!(
            AI_NOTICE_FORM_B.contains(AI_NOTICE_FORM_B_MARKER),
            "Form B fragment must carry its marker so auto-injection is idempotent"
        );
    }
}
