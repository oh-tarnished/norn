//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Writes a starting corpus per fuzz target under the directory named first on
//! the command line, from the writers the fixtures of Norn's tests use.

mod codecs;
mod files;
mod index;
mod wire;

use std::path::{Path, PathBuf};

/// Where seeds go: `<root>/<target>/<name>`.
pub struct Corpus(PathBuf);

impl Corpus {
    /// Writes one seed, the selector byte in front where a target takes one.
    pub fn put(&self, target: &str, name: &str, bytes: &[u8]) {
        let dir = self.0.join(target);
        std::fs::create_dir_all(&dir).expect("a corpus directory");
        std::fs::write(dir.join(name), bytes).expect("a seed");
    }

    /// As [`Self::put`], behind a selector byte.
    pub fn selected(&self, target: &str, name: &str, selector: u8, bytes: &[u8]) {
        let mut seed = vec![selector];
        seed.extend_from_slice(bytes);
        self.put(target, name, &seed);
    }

    fn root(&self) -> &Path {
        &self.0
    }
}

fn main() {
    let root = std::env::args_os()
        .nth(1)
        .expect("usage: norn-fuzz-seed <corpus dir>");
    let corpus = Corpus(PathBuf::from(root));
    codecs::write(&corpus);
    files::write(&corpus);
    index::write(&corpus);
    wire::write(&corpus);
    println!("seeds written under {}", corpus.root().display());
}
