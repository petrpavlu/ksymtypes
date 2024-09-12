// Copyright (C) 2024 SUSE LLC <petr.pavlu@suse.com>
// SPDX-License-Identifier: GPL-2.0-or-later

use ksyms::sym::SymCorpus;
use log::debug;
use std::path::Path;
use std::time::Instant;
use std::{env, process};

/// A type to measure elapsed time for some operation.
///
/// The time is measured between when the object is instantiated and when it is dropped. A message
/// with the elapsed time is output when the object is dropped.
enum Timing {
    Active { desc: String, start: Instant },
    Inactive,
}

impl Timing {
    fn new(do_timing: bool, desc: &str) -> Self {
        if do_timing {
            Timing::Active {
                desc: desc.to_string(),
                start: Instant::now(),
            }
        } else {
            Timing::Inactive
        }
    }
}

impl Drop for Timing {
    fn drop(&mut self) {
        match self {
            Timing::Active { desc, start } => {
                eprintln!("{}: {:.3?}", desc, start.elapsed());
            }
            Timing::Inactive => {}
        }
    }
}

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
fn do_consolidate<I>(program: &str, do_timing: bool, args: I) -> Result<(), ()>
where
    I: IntoIterator<Item = String>,
{
    // Parse specific command options.
    let mut output = "-".to_string();
    let mut maybe_path = None;
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
        if maybe_path.is_none() {
            maybe_path = Some(arg);
            continue;
        }
        eprintln!("Excess consolidate argument '{}' specified", arg);
        return Err(());
    }

    let path = match maybe_path {
        Some(path) => path,
        None => {
            eprintln!("The consolidate source is missing");
            return Err(());
        }
    };

    // Do the consolidation.
    debug!("Consolidate '{}' to '{}'", path, output);

    let mut syms = {
        let timing = Timing::new(do_timing, &format!("Reading symtypes from '{}'", path));

        let mut syms = SymCorpus::new();
        match syms.load(&Path::new(&path)) {
            Ok(_) => {}
            Err(err) => {
                eprintln!("Failed to read symtypes from '{}': {}", path, err);
                return Err(());
            }
        }
        syms
    };

    {
        let timing = Timing::new(
            do_timing,
            &format!("Writing consolidated symtypes to '{}'", output),
        );

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
    }

    Ok(())
}

/// Handles the `compare` command which shows differences between two symtypes corpuses.
fn do_compare<I>(program: &str, do_timing: bool, args: I) -> Result<(), ()>
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

    let mut syms1 = {
        let timing = Timing::new(do_timing, &format!("Reading symtypes from '{}'", path1));

        let mut syms1 = SymCorpus::new();
        match syms1.load(&Path::new(&path1)) {
            Ok(_) => {}
            Err(err) => {
                eprintln!("Failed to read symtypes from '{}': {}", path1, err);
                return Err(());
            }
        }
        syms1
    };

    let mut syms2 = {
        let timing = Timing::new(do_timing, &format!("Reading symtypes from '{}'", path1));

        let mut syms2 = SymCorpus::new();
        match syms2.load(&Path::new(&path2)) {
            Ok(_) => {}
            Err(err) => {
                eprintln!("Failed to read symtypes from '{}': {}", path2, err);
                return Err(());
            }
        }
        syms2
    };

    {
        let timing = Timing::new(do_timing, "Comparison");

        syms1.compare_with(&syms2);
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
    let mut do_timing = false;
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
            do_timing = true;
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
            if let Err(_) = do_consolidate(&program, do_timing, args) {
                process::exit(1);
            }
        }
        "compare" => {
            if let Err(_) = do_compare(&program, do_timing, args) {
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
