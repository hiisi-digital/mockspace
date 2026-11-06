use std::collections::BTreeSet;
use std::path::Path;
use mockspace_extra_lints::tools::near_copies::NearCopies;
use mockspace_lint_rules::tool::{Tool, ToolContext};
use mockspace_lint_rules::RegistryView;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let args: Vec<&str> = a.iter().map(String::as_str).collect();
    let root = Path::new("/home/user/kaski");
    let mut crates = BTreeSet::new();
    for dir in ["mock/crates", "mock/tools"] {
        if let Ok(rd) = std::fs::read_dir(root.join(dir)) {
            for e in rd.flatten() { if e.path().is_dir() { crates.insert(e.file_name().to_string_lossy().into_owned()); } }
        }
    }
    let mock = root.join("mock");
    let reg = RegistryView::default();
    let ctx = ToolContext { mock_dir: &mock, repo_root: root, all_crates: &crates, src_dirs: &[], args: &args, stdin: None, registry: &reg };
    let r = NearCopies.run(&ctx);
    println!("{}", r.output);
    println!("outcome: {:?}", match r.outcome { mockspace_lint_rules::Outcome::Clean{examined} => format!("clean examined={examined}"), mockspace_lint_rules::Outcome::Inconclusive{reason} => format!("inconclusive {reason}"), _ => "findings".into() });
}
