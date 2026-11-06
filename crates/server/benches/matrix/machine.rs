//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! What a run ran on and from: the machine, the disk under the data root, and
//! the commit. Best effort, through the platform's own tools; a field it cannot
//! learn reads "unknown" rather than failing the run, and a number without its
//! machine is not reported at all.

use std::path::Path;
use std::process::Command;

use serde_json::{Value, json};

/// A command's trimmed standard output, if it ran and succeeded.
fn out(program: &str, args: &[&str]) -> Option<String> {
    let o = Command::new(program).args(args).output().ok()?;
    let text = String::from_utf8_lossy(&o.stdout).trim().to_owned();
    (o.status.success() && !text.is_empty()).then_some(text)
}

/// The value after `key:` on the first line that has it.
fn field(text: &str, key: &str) -> Option<String> {
    text.lines()
        .find_map(|l| l.trim().strip_prefix(key)?.trim().strip_prefix(':'))
        .map(|v| v.trim().trim_matches('"').to_owned())
}

fn known(v: Option<String>) -> String {
    v.unwrap_or_else(|| "unknown".into())
}

/// The device holding `dir`, per `df -P`.
fn device(dir: &Path) -> Option<String> {
    let text = out("df", &["-P", &dir.to_string_lossy()])?;
    text.lines()
        .nth(1)?
        .split_whitespace()
        .next()
        .map(str::to_owned)
}

#[cfg(target_os = "macos")]
fn platform(dir: &Path) -> [String; 5] {
    let sys = |k: &str| out("sysctl", &["-n", k]);
    let info = device(dir).and_then(|d| out("diskutil", &["info", &d]));
    let info = info.as_deref().unwrap_or("");
    // An APFS volume names its media on the whole disk it is part of.
    let whole = field(info, "Part of Whole").and_then(|d| out("diskutil", &["info", &d]));
    let media = whole
        .as_deref()
        .and_then(|w| field(w, "Device / Media Name"));
    let os = out("sw_vers", &["-productVersion"]).map(|v| format!("macOS {v}"));
    [
        known(os),
        known(sys("machdep.cpu.brand_string")),
        known(sys("hw.memsize")),
        known(media),
        known(field(info, "File System Personality")),
    ]
}

#[cfg(not(target_os = "macos"))]
fn platform(dir: &Path) -> [String; 5] {
    let read = |p: &str| std::fs::read_to_string(p).ok();
    let os = read("/etc/os-release").and_then(|t| {
        let line = t
            .lines()
            .find(|l| l.starts_with("PRETTY_NAME="))?
            .to_owned();
        Some(
            line.trim_start_matches("PRETTY_NAME=")
                .trim_matches('"')
                .to_owned(),
        )
    });
    let cpu = read("/proc/cpuinfo").and_then(|t| field(&t, "model name"));
    let ram = read("/proc/meminfo")
        .and_then(|t| field(&t, "MemTotal"))
        .and_then(|v| v.trim_end_matches("kB").trim().parse::<u64>().ok())
        .map(|kib| (kib * 1024).to_string());
    let dir = dir.to_string_lossy();
    let fs = out("findmnt", &["-no", "FSTYPE", "-T", &dir]);
    let parent = device(Path::new(&*dir)).and_then(|d| out("lsblk", &["-no", "PKNAME", &d]));
    let disk = parent.and_then(|p| {
        out(
            "lsblk",
            &["-dno", "MODEL", &format!("/dev/{}", p.lines().next()?)],
        )
    });
    [known(os), known(cpu), known(ram), known(disk), known(fs)]
}

/// The machine, and the disk under `data_root`.
pub fn machine(data_root: &Path) -> Value {
    let [os, cpu, ram, disk, fs] = platform(data_root);
    let cores = std::thread::available_parallelism().map_or(0, std::num::NonZero::get);
    json!({
        "os": os,
        "kernel": known(out("uname", &["-sr"])),
        "arch": std::env::consts::ARCH,
        "cpu": cpu,
        "logical_cpus": cores,
        "ram_bytes": ram.parse::<u64>().ok(),
        "data_root": data_root.display().to_string(),
        "disk_model": disk,
        "filesystem": fs,
        "rustc": known(out("rustc", &["-V"])),
    })
}

/// The commit, and whether the tree had uncommitted changes.
pub fn commit() -> Value {
    let dirty = out("git", &["status", "--porcelain", "--untracked-files=no"]).is_some();
    json!({
        "commit": known(out("git", &["rev-parse", "HEAD"])),
        "describe": known(out("git", &["describe", "--tags", "--always", "--dirty"])),
        "dirty": dirty,
    })
}
