//! A scratch world for the store's tests: a bare repository on the filesystem and
//! any number of clone directories, all driven with plain git.

#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::process::Command;

use mockspace_store::{Edit, MergeHook, MergeRefusal, Reader, Reapply, RefStore, Store};

pub const EMPTY_TREE: &str = "4b825dc642cb6eb9a060e54bf8d69288fbee4904";

pub const T1: &str = "id = \"t1\"\n";
pub const T2: &str = "id = \"t2\"\n";

/// A clone named `a` whose store holds a ref of two files, pushed.
pub fn seeded(w: &World) -> RefStore {
    let a = store(w, "a");
    a.apply(
        &Edit::new()
            .put("ticket/t1.toml", T1)
            .put("ticket/t2.toml", T2),
        "seed",
        &Reapply,
    )
    .unwrap();
    a
}

/// Where an instance of a clone goes: `.mock/<leaf>` in it.
pub fn inst_dir(w: &World, clone: &str, leaf: &str) -> PathBuf {
    w.dir.path().join(clone).join(".mock").join(leaf)
}

pub fn read(dir: &Path, rel: &str) -> String {
    std::fs::read_to_string(dir.join(rel)).unwrap()
}

pub fn write(dir: &Path, rel: &str, text: &str) {
    let path = dir.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

pub fn git(dir: &Path, args: &[&str]) -> String {
    try_git(dir, args).unwrap_or_else(|e| panic!("git {args:?} in {}: {e}", dir.display()))
}

pub fn try_git(dir: &Path, args: &[&str]) -> Result<String, String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env_remove("GIT_DIR")
        .env_remove("GIT_INDEX_FILE")
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@example.invalid")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@example.invalid")
        .output()
        .expect("spawn git");
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_owned())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_owned())
    }
}

pub struct World {
    pub dir:  tempfile::TempDir,
    pub bare: PathBuf,
}

impl World {
    pub fn new() -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        let bare = dir.path().join("remote.git");
        std::fs::create_dir(&bare).unwrap();
        git(&bare, &["init", "-q", "--bare"]);
        Self {
            dir,
            bare,
        }
    }

    /// A fresh repository with no remotes, for one writer.
    pub fn clone_dir(&self, name: &str) -> PathBuf {
        let p = self.dir.path().join(name);
        std::fs::create_dir(&p).unwrap();
        git(&p, &["init", "-q"]);
        p
    }

    pub fn remote(&self) -> String {
        self.bare.to_string_lossy().into_owned()
    }

    pub fn tip(&self, reference: &str) -> Option<String> {
        try_git(&self.bare, &["rev-parse", "-q", "--verify", reference])
            .ok()
            .filter(|s| !s.is_empty())
    }

    pub fn commits(&self, reference: &str) -> usize {
        git(&self.bare, &["rev-list", "--count", reference])
            .parse()
            .unwrap()
    }

    pub fn file(&self, reference: &str, path: &str) -> Option<String> {
        try_git(&self.bare, &["show", &format!("{reference}:{path}")]).ok()
    }

    /// A commit on top of `parent` in the bare repository holding one file, written
    /// by another writer that is not a store; used to win a race on purpose.
    pub fn competing_commit(&self, parent: &str, path: &str, content: &str) -> String {
        let idx = self.dir.path().join("competitor.idx");
        let run = |args: &[&str], input: Option<&str>| -> String {
            use std::io::Write;
            let mut cmd = Command::new("git");
            cmd.arg("-C")
                .arg(&self.bare)
                .args(args)
                .env("GIT_INDEX_FILE", &idx)
                .env("GIT_AUTHOR_NAME", "rival")
                .env("GIT_AUTHOR_EMAIL", "rival@example.invalid")
                .env("GIT_COMMITTER_NAME", "rival")
                .env("GIT_COMMITTER_EMAIL", "rival@example.invalid");
            cmd.stdin(if input.is_some() {
                std::process::Stdio::piped()
            } else {
                std::process::Stdio::null()
            })
            .stdout(std::process::Stdio::piped());
            let mut child = cmd.spawn().unwrap();
            if let Some(i) = input {
                child.stdin.take().unwrap().write_all(i.as_bytes()).unwrap();
            }
            let o = child.wait_with_output().unwrap();
            assert!(o.status.success(), "git {args:?}");
            String::from_utf8_lossy(&o.stdout).trim().to_owned()
        };
        run(&["read-tree", parent], None);
        let blob = run(&["hash-object", "-w", "--stdin"], Some(content));
        run(
            &["update-index", "--add", "--cacheinfo", &format!("100644,{blob},{path}")],
            None,
        );
        let tree = run(&["write-tree"], None);
        run(&["commit-tree", &tree, "-p", parent, "-m", "rival"], None)
    }

    pub fn install_hook(&self, repo: &Path, name: &str, body: &str) {
        use std::os::unix::fs::PermissionsExt;
        let hooks = repo.join(".git/hooks");
        let hooks = if hooks.is_dir() { hooks } else { repo.join("hooks") };
        let file = hooks.join(name);
        std::fs::write(&file, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
}

/// A merge hook that records the tip it was shown and then reapplies the edit.
#[derive(Default)]
pub struct Recording {
    pub tips: std::sync::Mutex<Vec<Vec<String>>>,
}

impl MergeHook for Recording {
    fn rebase(&self, tip: &dyn Reader, ours: &Edit) -> Result<Edit, MergeRefusal> {
        self.tips.lock().unwrap().push(tip.list("").unwrap());
        Ok(ours.clone())
    }
}

pub const NS: &str = "registry/probe";
pub const MOCK_REF: &str = "refs/mock/registry/probe";

/// A ref store in a fresh clone directory, on the ref form of `NS`.
pub fn store(w: &World, name: &str) -> mockspace_store::RefStore {
    mockspace_store::RefStore::new(
        w.clone_dir(name),
        w.remote(),
        mockspace_store::RefPair::ref_form(NS).unwrap(),
    )
    .with_retry(mockspace_store::RetryPolicy::immediate(4))
}

/// Adds one to the `counter` file of the tip that won, whatever the edit was.
pub struct Bump;

impl MergeHook for Bump {
    fn rebase(&self, tip: &dyn Reader, _ours: &Edit) -> Result<Edit, MergeRefusal> {
        let n: u32 = String::from_utf8(tip.read("counter").unwrap().unwrap())
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        Ok(Edit::new().put("counter", format!("{}\n", n + 1)))
    }
}

/// A stand-in for the `git` command: it runs the real one, and for `push` and for `fetch` it
/// first runs the matching shell snippet (empty for none), which may `exit`. `"$@"` is the real
/// arguments.
pub fn git_shim(w: &World, on_push: &str, on_fetch: &str) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let p = w.dir.path().join("git-shim");
    let body = format!(
        "#!/bin/sh\nfor a in \"$@\"; do\n  if [ \"$a\" = push ]; then\n  :\n{on_push}\n  fi\n  if [ \"$a\" = fetch ]; then\n  :\n{on_fetch}\n  fi\ndone\nexec git \"$@\"\n"
    );
    std::fs::write(&p, body).unwrap();
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    p
}
