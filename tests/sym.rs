// Copyright (C) 2024 SUSE LLC <petr.pavlu@suse.com>
// SPDX-License-Identifier: GPL-2.0-or-later

use ksymtypes::sym::SymCorpus;
use std::path::Path;

macro_rules! assert_parse_err {
    ($result:expr, $exp_desc:expr) => {
        match $result {
            Err(ksymtypes::Error::Parse(actual_desc)) => assert_eq!(actual_desc, $exp_desc),
            result => panic!(
                "assertion failed: {:?} is not of type Err(ksymtypes::Error::Parse())",
                result
            ),
        }
    };
}

#[test]
fn read_empty_record() {
    // Check that empty records are rejected when reading a file.
    let input = concat!(
        "s#test struct test { }\n",
        "\n",
        "s#test2 struct test2 { }\n", //
    );
    let mut syms = SymCorpus::new();
    let result = syms.load_buffer(&Path::new("file.symtypes"), input.as_bytes());
    assert_parse_err!(result, "file.symtypes:2: Expected a record name");
}

#[test]
fn read_duplicate_type_record() {
    // Check that type records with duplicate names are rejected when reading a file.
    let input = concat!(
        "s#test struct test { int a ; }\n",
        "s#test struct test { int b ; }\n", //
    );
    let mut syms = SymCorpus::new();
    let result = syms.load_buffer(&Path::new("file.symtypes"), input.as_bytes());
    assert_parse_err!(result, "file.symtypes:2: Duplicate record 's#test'");
}

#[test]
fn read_duplicate_file_record() {
    // Check that F# records with duplicate names are rejected when reading a consolidated file.
    let input = concat!(
        "bar int bar ( )\n",
        "baz int baz ( )\n",
        "F#test.symtypes bar\n",
        "F#test.symtypes baz\n", //
    );
    let mut syms = SymCorpus::new();
    let result = syms.load_buffer(&Path::new("file.symtypes"), input.as_bytes());
    assert_parse_err!(
        result,
        "file.symtypes:4: Duplicate record 'F#test.symtypes'"
    );
}

#[test]
fn read_invalid_file_record_ref() {
    // Check that an F# record referencing a type in form '<base_name>' is rejected if the type is
    // not known.
    let input = concat!(
        "F#test.symtypes bar\n", //
    );
    let mut syms = SymCorpus::new();
    let result = syms.load_buffer(&Path::new("file.symtypes"), input.as_bytes());
    assert_parse_err!(result, "file.symtypes:1: Type bar is not known");
}

#[test]
fn read_invalid_file_record_ref2() {
    // Check that an F# record referencing a type in form '<base_name>@<variant_idx>' is rejected if
    // the base name is not known.
    let input = concat!(
        "F#test.symtypes bar@0\n", //
    );
    let mut syms = SymCorpus::new();
    let result = syms.load_buffer(&Path::new("file.symtypes"), input.as_bytes());
    assert_parse_err!(result, "file.symtypes:1: Type bar@0 is not known");
}

#[test]
fn read_invalid_file_record_ref3() {
    // Check that an F# record referencing a type in form '<base_name>@<variant_idx>' is rejected if
    // the variant index is not known.
    let input = concat!(
        "bar@0 int bar ( )\n",
        "F#test.symtypes bar@0\n",  //
        "F#test2.symtypes bar@1\n", //
    );
    let mut syms = SymCorpus::new();
    let result = syms.load_buffer(&Path::new("file.symtypes"), input.as_bytes());
    assert_parse_err!(result, "file.symtypes:3: Type bar@1 is not known");
}

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
