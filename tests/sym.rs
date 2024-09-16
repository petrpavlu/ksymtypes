// Copyright (C) 2024 SUSE LLC <petr.pavlu@suse.com>
// SPDX-License-Identifier: GPL-2.0-or-later

use ksymtypes::sym::SymCorpus;
use std::path::Path;

#[test]
fn read_write_basic() {
    // Check reading of a single file and writing the consolidated output.
    let mut syms = SymCorpus::new();
    syms.load_buffer(
        Path::new("test.symtypes"),
        concat!(
            "s#foo struct foo { int a ; }\n",
            "bar int bar ( s#foo )\n", //
        )
        .as_bytes(),
    );
    let mut out = Vec::new();
    syms.write_consolidated(&mut out);
    assert_eq!(
        String::from_utf8(out).unwrap(),
        concat!(
            "s#foo struct foo { int a ; }\n",
            "bar int bar ( s#foo )\n",
            "F#test.symtypes bar\n", //
        )
    );
}

#[test]
fn read_write_shared_struct() {
    // Check that a structure declaration shared by two files appears only once in the consolidated
    // output.
    let mut syms = SymCorpus::new();
    syms.load_buffer(
        Path::new("test.symtypes"),
        concat!(
            "s#foo struct foo { int a ; }\n",
            "bar int bar ( s#foo )\n", //
        )
        .as_bytes(),
    );
    syms.load_buffer(
        Path::new("test2.symtypes"),
        concat!(
            "s#foo struct foo { int a ; }\n",
            "baz int baz ( s#foo )\n", //
        )
        .as_bytes(),
    );
    let mut out = Vec::new();
    syms.write_consolidated(&mut out);
    assert_eq!(
        String::from_utf8(out).unwrap(),
        concat!(
            "s#foo struct foo { int a ; }\n",
            "bar int bar ( s#foo )\n",
            "baz int baz ( s#foo )\n",
            "F#test.symtypes bar\n",
            "F#test2.symtypes baz\n", //
        )
    );
}

#[test]
fn read_write_differing_struct() {
    // Check that a structure declaration different in two files appears in all variants in the
    // consolidated output and they are correctly referenced by the F# entries.
    let mut syms = SymCorpus::new();
    syms.load_buffer(
        Path::new("test.symtypes"),
        concat!(
            "s#foo struct foo { int a ; }\n",
            "bar int bar ( s#foo )\n", //
        )
        .as_bytes(),
    );
    syms.load_buffer(
        Path::new("test2.symtypes"),
        concat!(
            "s#foo struct foo { UNKNOWN }\n",
            "baz int baz ( s#foo )\n", //
        )
        .as_bytes(),
    );
    let mut out = Vec::new();
    syms.write_consolidated(&mut out);
    assert_eq!(
        String::from_utf8(out).unwrap(),
        concat!(
            "s#foo@0 struct foo { int a ; }\n",
            "s#foo@1 struct foo { UNKNOWN }\n",
            "bar int bar ( s#foo )\n",
            "baz int baz ( s#foo )\n",
            "F#test.symtypes s#foo@0 bar\n",
            "F#test2.symtypes s#foo@1 baz\n", //
        )
    );
}
