// Copyright (C) 2024 SUSE LLC <petr.pavlu@suse.com>
// SPDX-License-Identifier: GPL-2.0-or-later

use std::env;
use std::fs;
use std::path::Path;
use std::process::Command;

fn main() -> std::io::Result<()> {
    let out_dir = env::var_os("OUT_DIR").unwrap();
    let out_dir = Path::new(&out_dir);

    // Build man pages from the Markdown files.
    let man_out_dir = out_dir.join("doc");
    if !man_out_dir.exists() {
        if let Err(err) = fs::create_dir(out_dir.join("doc")) {
            eprintln!(
                "Failed to create the directory '{}': {}",
                man_out_dir.display(),
                err
            );
            return Err(err);
        }
    }

    let man_srcs = ["doc/ksymtypes.1.md", "doc/ksymtypes.5.md"];
    for man_src in man_srcs {
        let man_dst = out_dir.join(&man_src[..man_src.len() - 3]);
        let status = match Command::new("pandoc")
            .args([
                "--standalone",
                "--fail-if-warnings=true",
                "--from=markdown",
                "--to=man",
                "--output",
            ])
            .arg(&man_dst)
            .arg(&man_src)
            .status()
        {
            Ok(status) => status,
            Err(err) => {
                eprintln!(
                    "Failed to execute pandoc convert '{}' to '{}': {}",
                    man_src,
                    man_dst.display(),
                    err
                );
                return Err(err);
            }
        };

        if !status.success() {
            eprintln!(
                "Failed to execute pandoc to convert '{}' to '{}': {}",
                man_src,
                man_dst.display(),
                status
            );
            return Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("Pandoc conversion failed: {}", status),
            ));
        }

        println!("cargo::rerun-if-changed={}", man_src);
    }

    println!("cargo::rerun-if-changed=build.rs");
    Ok(())
}
