//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! Which disassembler this host has, found once, and how each flavour is
//! asked for a whole file or for one symbol.
//!
//! The order of preference is `llvm-objdump` on `PATH` (also as a versioned
//! `llvm-objdump-<N>`, the highest), the `llvm-objdump` of the toolchain's
//! `llvm-tools` component, then `objdump` classified by its own `--version`
//! (Apple's and a renamed LLVM one say LLVM, binutils says GNU), then `otool`
//! and `dumpbin`. One tool serves every file of a run, so two variants are
//! never compared across two tools' output.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

use super::format::{ObjectFormat, symbol_spellings};
use super::pe_exports::export_block;

/// The output dialects the duplicate check reads. A fixed vocabulary of tools,
/// not content: each is a different program with its own flags and its own
/// line shape.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Flavour {
    /// `llvm-objdump`, and Apple's `objdump`, which is LLVM's.
    Llvm,
    /// GNU binutils' `objdump`.
    Gnu,
    /// macOS `otool`, which disassembles `__TEXT,__text` only and has no
    /// symbol filter.
    Otool,
    /// MSVC's `dumpbin`, which has no symbol filter.
    Dumpbin,
}

/// One disassembler program and the flavour its output is read as.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Disassembler {
    /// The program to run.
    pub program: PathBuf,
    /// How it is asked and how its output is read.
    pub flavour: Flavour,
}

impl Disassembler {
    /// The disassembler of this host, found on first use and kept for the
    /// process, or `None` where it has none.
    pub fn detected() -> Option<&'static Disassembler> {
        static FOUND: OnceLock<Option<Disassembler>> = OnceLock::new();
        FOUND.get_or_init(detect).as_ref()
    }

    /// The arguments that disassemble every executable section of `file`,
    /// without the raw instruction bytes where the tool can leave them out.
    fn whole_args(&self, file: &Path) -> Vec<String> {
        let file = file.display().to_string();
        match self.flavour {
            Flavour::Llvm | Flavour::Gnu => vec!["-d".into(), "--no-show-raw-insn".into(), file],
            Flavour::Otool => vec!["-tv".into(), file],
            Flavour::Dumpbin => vec!["/NOLOGO".into(), "/DISASM:NOBYTES".into(), file],
        }
    }

    /// The arguments that disassemble only `spelling`, where the tool can
    /// filter by symbol. `None` for a tool that cannot, whose whole output
    /// is cut to the symbol instead.
    fn symbol_args(&self, file: &Path, spelling: &str) -> Option<Vec<String>> {
        let flag = match self.flavour {
            Flavour::Llvm => format!("--disassemble-symbols={spelling}"),
            Flavour::Gnu => format!("--disassemble={spelling}"),
            Flavour::Otool | Flavour::Dumpbin => return None,
        };
        Some(vec![
            "-d".into(),
            "--no-show-raw-insn".into(),
            flag,
            file.display().to_string(),
        ])
    }

    fn run(&self, args: &[String]) -> Result<String, String> {
        let out = Command::new(&self.program)
            .args(args)
            .output()
            .map_err(|e| format!("{} could not be run: {e}", self.program.display()))?;
        if !out.status.success() {
            return Err(format!(
                "{} {} exited {}: {}",
                self.program.display(),
                args.join(" "),
                out.status,
                String::from_utf8_lossy(&out.stderr).trim()
            ));
        }
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    }

    /// The raw disassembly of every executable section of `file`.
    pub fn whole(&self, file: &Path) -> Result<String, String> {
        self.run(&self.whole_args(file))
    }

    /// The raw disassembly of the function `symbol` (its name in C) in `file`,
    /// from its label to the end of its body, spelled as the file's object
    /// format spells it.
    pub fn symbol(&self, file: &Path, symbol: &str) -> Result<String, String> {
        let spellings = symbol_spellings(file, symbol);
        let mut whole: Option<String> = None;
        for spelling in &spellings {
            let text = match self.symbol_args(file, spelling) {
                Some(args) => self.run(&args)?,
                None => {
                    match &whole {
                        Some(text) => text.clone(),
                        None => whole.insert(self.whole(file)?).clone(),
                    }
                },
            };
            if let Some(block) = symbol_block(&text, spelling) {
                return Ok(block);
            }
        }
        if self.flavour == Flavour::Gnu
            && matches!(
                ObjectFormat::of_file(file),
                Some(ObjectFormat::Coff {
                    image: true,
                    ..
                })
            )
        {
            let p = self.run(&["-p".to_string(), file.display().to_string()])?;
            let whole = self.whole(file)?;
            for spelling in &spellings {
                if let Some(block) = export_block(&whole, &p, spelling) {
                    return Ok(block);
                }
            }
        }
        Err(format!(
            "{} has no function {} in {}",
            self.program.display(),
            spellings.join(" or "),
            file.display()
        ))
    }
}

/// Whether `line` is a symbol's label: `<sym>:` or `addr <sym>:` (objdump),
/// or `sym:` alone (otool, dumpbin).
fn is_label(trimmed: &str) -> bool {
    let Some(head) = trimmed.strip_suffix(':') else { return false };
    head.ends_with('>') || !head.contains(char::is_whitespace)
}

/// Whether the label line `trimmed` names exactly `spelling`.
fn labels(trimmed: &str, spelling: &str) -> bool {
    let Some(head) = trimmed.strip_suffix(':') else { return false };
    head == spelling
        || head
            .strip_suffix('>')
            .and_then(|h| h.rsplit_once('<'))
            .is_some_and(|(addr, name)| name == spelling && !addr.contains('>'))
}

/// The lines of `text` from the label of `spelling` to the next label or
/// section header, the label included, or `None` where it has no such label.
pub(super) fn symbol_block(text: &str, spelling: &str) -> Option<String> {
    let mut lines = text.lines();
    let label = lines.by_ref().find(|l| labels(l.trim(), spelling))?;
    let mut block = vec![label];
    for line in lines {
        let trimmed = line.trim();
        if is_label(trimmed) || trimmed.starts_with("Disassembly of section") {
            break;
        }
        block.push(line);
    }
    while block.last().is_some_and(|l| l.trim().is_empty()) {
        block.pop();
    }
    Some(block.join("\n"))
}

/// What a probe of `program --version` (or of running it at all) says about
/// the flavour it is, or `None` where it could not be run.
fn classify(program: &Path) -> Option<Flavour> {
    let out = Command::new(program).arg("--version").output().ok()?;
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    classify_version(&text)
}

/// The flavour a `--version` text names: LLVM's (Apple's included) or GNU's.
pub(super) fn classify_version(text: &str) -> Option<Flavour> {
    if text.contains("LLVM") {
        Some(Flavour::Llvm)
    } else if text.contains("GNU") {
        Some(Flavour::Gnu)
    } else {
        None
    }
}

/// Every directory on `PATH`.
fn path_dirs() -> Vec<PathBuf> {
    std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).collect())
        .unwrap_or_default()
}

/// `llvm-objdump` on `PATH`, or the highest versioned `llvm-objdump-<N>`.
fn llvm_objdump_on_path() -> Option<PathBuf> {
    let exe = std::env::consts::EXE_SUFFIX;
    let mut versioned: Option<(u32, PathBuf)> = None;
    for dir in path_dirs() {
        let plain = dir.join(format!("llvm-objdump{exe}"));
        if plain.is_file() {
            return Some(plain);
        }
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            let name = entry.file_name();
            let Some(v) = name
                .to_str()
                .and_then(|n| n.strip_suffix(exe))
                .and_then(|n| n.strip_prefix("llvm-objdump-"))
                .and_then(|v| v.parse::<u32>().ok())
            else {
                continue;
            };
            if versioned.as_ref().is_none_or(|(best, _)| v > *best) {
                versioned = Some((v, entry.path()));
            }
        }
    }
    versioned.map(|(_, p)| p)
}

/// The `llvm-objdump` of the active toolchain's `llvm-tools` component.
fn llvm_objdump_of_toolchain() -> Option<PathBuf> {
    let rustc = std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into());
    let sysroot = Command::new(&rustc)
        .args(["--print", "sysroot"])
        .output()
        .ok()?;
    let verbose = Command::new(&rustc).arg("-vV").output().ok()?;
    let host = String::from_utf8_lossy(&verbose.stdout)
        .lines()
        .find_map(|l| l.strip_prefix("host: ").map(str::to_string))?;
    let path = PathBuf::from(String::from_utf8_lossy(&sysroot.stdout).trim())
        .join("lib/rustlib")
        .join(host)
        .join("bin")
        .join(format!("llvm-objdump{}", std::env::consts::EXE_SUFFIX));
    path.is_file().then_some(path)
}

/// Whether `program` can be spawned at all.
fn runs(program: &str, arg: &str) -> bool {
    Command::new(program).arg(arg).output().is_ok()
}

fn detect() -> Option<Disassembler> {
    let llvm = |program: PathBuf| {
        Disassembler {
            program,
            flavour: Flavour::Llvm,
        }
    };
    if let Some(p) = llvm_objdump_on_path() {
        return Some(llvm(p));
    }
    if let Some(p) = llvm_objdump_of_toolchain() {
        return Some(llvm(p));
    }
    if let Some(flavour) = classify(Path::new("objdump")) {
        return Some(Disassembler {
            program: "objdump".into(),
            flavour,
        });
    }
    if cfg!(target_os = "macos") && runs("otool", "-h") {
        return Some(Disassembler {
            program: "otool".into(),
            flavour: Flavour::Otool,
        });
    }
    if cfg!(windows) && runs("dumpbin", "/?") {
        return Some(Disassembler {
            program: "dumpbin".into(),
            flavour: Flavour::Dumpbin,
        });
    }
    None
}
