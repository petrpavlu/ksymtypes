// Copyright (C) 2024 SUSE LLC <petr.pavlu@suse.com>
// SPDX-License-Identifier: GPL-2.0-or-later

use getopts::Options;
use log::debug;
use ksyms::sym::SymTypes;
use std::{env, process};

fn print_usage(program: &str, opts: Options) {
    let brief = format!("Usage: {} [options] DIR1 DIR2", program);
    print!("{}", opts.usage(&brief));
}

fn compare_dirs(dir1: &str, dir2: &str) -> Result<(), ()> {
    debug!("Compare '{}' and '{}'", dir1, dir2);

    // TODO
    let s1 = match SymTypes::new(dir1) {
        Ok(s1) => s1,
        Err(err) => {
            eprintln!("Failed to read symtypes from '{}': {}", dir1, err);
            return Err(());
        }
    };
    let s2 = match SymTypes::new(dir2) {
        Ok(s2) => s2,
        Err(err) => {
            eprintln!("Failed to read symtypes from '{}': {}", dir2, err);
            return Err(());
        }
    };
    s1.compare_with(&s2);

    Ok(())
}

fn main() {
    env_logger::init();

    let args: Vec<String> = env::args().collect();
    let program = args[0].clone();

    let mut opts = Options::new();
    opts.optflag("h", "help", "print this help menu");
    let matches = match opts.parse(&args[1..]) {
        Ok(m) => m,
        Err(f) => {
            eprintln!("{}", f.to_string());
            process::exit(1);
        }
    };
    if matches.opt_present("h") {
        print_usage(&program, opts);
        process::exit(0);
    }

    if matches.free.len() != 2 {
        print_usage(&program, opts);
        process::exit(1);
    }

    let mut rc = 0;
    if let Err(_) = compare_dirs(&matches.free[0], &matches.free[1]) {
        rc = 1
    }

    process::exit(rc);
}
