// Copyright (C) 2024 SUSE LLC <petr.pavlu@suse.com>
// SPDX-License-Identifier: GPL-2.0-or-later

use ksyms::sym::SymCorpus;
use log::debug;
use std::path::Path;
use std::time::Instant;
use std::{env, process};

/// Prints the global usage message on `stdout`.
fn print_usage(program: &str) {
    print!(
        concat!(
            "Usage: {} [OPTIONS] COMMAND\n",
            "\n",
            "OPTIONS\n",
            "  -h, --help            print this help\n",
            "\n",
            "COMMAND\n",
            "  consolidate           consolidate symtypes into a single file\n",
            "  compare               show differences between two symtypes corpuses\n",
        ),
        program
    );
}

/// Prints the usage message for the `consolidate` command on `stdout`.
fn print_consolidate_usage(program: &str) {
    print!(
        concat!(
            "Usage: {} consolidate [OPTIONS] DIR\n",
            "Consolidate symtypes into a single file.\n",
            "\n",
            "OPTIONS\n",
            "  -h, --help            print this help\n",
            "  -o, --output=FILE     write the result in a specified file, instead of stdout\n",
        ),
        program
    );
}

/// Prints the usage message for the `compare` command on `stdout`.
fn print_compare_usage(program: &str) {
    print!(
        concat!(
            "Usage: {} compare [OPTIONS] DIR1 DIR2\n",
            "Show differences between two symtypes corpuses.\n",
            "\n",
            "OPTIONS\n",
            "  -h, --help            print this help\n",
        ),
        program
    );
}

/// Handles the `consolidate` command which consolidates symtypes into a single file.
fn do_consolidate<I>(program: &str, timing: bool, args: I) -> Result<(), ()>
where
    I: IntoIterator<Item = String>,
{
    // Parse specific command options.
    let mut output = "-".to_string();
    // TODO dir -> path for consistency with compare.
    let mut maybe_dir = None;
    for arg in args.into_iter() {
        if arg == "-h" || arg == "--help" {
            print_consolidate_usage(&program);
            return Ok(());
        }
        if arg == "-o" || arg == "--output" {
            // TODO Implement correctly.
            output = arg.to_string();
            continue;
        }
        if arg.starts_with("-") || arg.starts_with("--") {
            eprintln!("Unrecognized consolidate option '{}'", arg);
            return Err(());
        }
        if maybe_dir.is_none() {
            maybe_dir = Some(arg);
            continue;
        }
        eprintln!("Excess consolidate argument '{}' specified", arg);
        return Err(());
    }

    let dir = match maybe_dir {
        Some(dir) => dir,
        None => {
            eprintln!("The consolidate source is missing");
            return Err(());
        }
    };

    // Do the comparison.
    debug!("Consolidate '{}' to '{}'", dir, output);

    let mut syms = SymCorpus::new();
    match syms.load_dir(&Path::new(&dir)) {
        Ok(_) => {}
        Err(err) => {
            eprintln!("Failed to read symtypes from '{}': {}", dir, err);
            return Err(());
        }
    };
    match syms.write_consolidated_file(&output) {
        Ok(_) => {}
        Err(err) => {
            eprintln!(
                "Failed to write consolidated symtypes to '{}': {}",
                output, err
            );
            return Err(());
        }
    }

    Ok(())
}

/// Handles the `compare` command which shows differences between two symtypes corpuses.
fn do_compare<I>(program: &str, timing: bool, args: I) -> Result<(), ()>
where
    I: IntoIterator<Item = String>,
{
    // Parse specific command options.
    let mut maybe_path1 = None;
    let mut maybe_path2 = None;
    for arg in args.into_iter() {
        if arg == "-h" || arg == "--help" {
            print_compare_usage(&program);
            return Ok(());
        }
        if arg.starts_with("-") || arg.starts_with("--") {
            eprintln!("Unrecognized compare option '{}'", arg);
            return Err(());
        }
        if maybe_path1.is_none() {
            maybe_path1 = Some(arg);
            continue;
        }
        if maybe_path2.is_none() {
            maybe_path2 = Some(arg);
            continue;
        }
        eprintln!("Excess compare argument '{}' specified", arg);
        return Err(());
    }

    let path1 = match maybe_path1 {
        Some(path1) => path1,
        None => {
            eprintln!("The first compare source is missing");
            return Err(());
        }
    };
    let path2 = match maybe_path2 {
        Some(path2) => path2,
        None => {
            eprintln!("The second compare source is missing");
            return Err(());
        }
    };

    // Do the comparison.
    debug!("Compare '{}' and '{}'", path1, path2);

    let mut now = None;
    if timing {
        now = Some(Instant::now());
    }
    let mut s1 = SymCorpus::new();
    match s1.load(&Path::new(&path1)) {
        Ok(_) => {}
        Err(err) => {
            eprintln!("Failed to read symtypes from '{}': {}", path1, err);
            return Err(());
        }
    };
    if timing {
        println!(
            "Reading symtypes from '{}' took {:.3?}",
            path1,
            now.unwrap().elapsed()
        );
    }

    if timing {
        now = Some(Instant::now());
    }
    let mut s2 = SymCorpus::new();
    match s2.load(&Path::new(&path2)) {
        Ok(_) => {}
        Err(err) => {
            eprintln!("Failed to read symtypes from '{}': {}", path2, err);
            return Err(());
        }
    };
    if timing {
        println!(
            "Reading symtypes from '{}' took {:.3?}",
            path2,
            now.unwrap().elapsed()
        );
    }

    if timing {
        now = Some(Instant::now());
    }
    s1.compare_with(&s2);
    if timing {
        println!("Comparison took {:.3?}", now.unwrap().elapsed());
    }

    Ok(())
}

fn main() {
    env_logger::init();

    let mut args = env::args();

    let program = match args.next() {
        Some(program) => program,
        None => {
            eprintln!("Unknown program name");
            process::exit(1);
        }
    };

    // Handle global options and stop at the command.
    let mut maybe_command = None;
    let mut timing = false;
    loop {
        let arg = match args.next() {
            Some(arg) => arg,
            None => break,
        };

        if arg == "-h" || arg == "--help" {
            print_usage(&program);
            process::exit(0);
        }
        if arg == "--timing" {
            timing = true;
            continue;
        }
        if arg.starts_with("-") || arg.starts_with("--") {
            eprintln!("Unrecognized global option '{}'", arg);
            process::exit(1);
        }
        maybe_command = Some(arg);
        break;
    }

    let command = match maybe_command {
        Some(command) => command,
        None => {
            eprintln!("No command specified");
            process::exit(1);
        }
    };

    // Process the specified command.
    match command.as_str() {
        "consolidate" => {
            if let Err(_) = do_consolidate(&program, timing, args) {
                process::exit(1);
            }
        }
        "compare" => {
            if let Err(_) = do_compare(&program, timing, args) {
                process::exit(1);
            }
        }
        _ => {
            eprintln!("Unrecognized command '{}'", command);
            process::exit(1);
        }
    }

    process::exit(0);
}
