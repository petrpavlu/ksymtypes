// Copyright (C) 2024 SUSE LLC <petr.pavlu@suse.com>
// SPDX-License-Identifier: GPL-2.0-or-later

use super::*;

#[test]
fn format_typedef() {
    // Check pretty-formatting of a typedef declaration.
    let pretty = pretty_format_type(&vec![
        Token::new_atom("typedef"),
        Token::new_atom("unsigned"),
        Token::new_atom("long"),
        Token::new_atom("long"),
        Token::new_atom("u64"),
    ]);
    assert_eq!(
        pretty,
        crate::string_vec!(
                "typedef unsigned long long u64" //
            )
    );
}

#[test]
fn format_enum() {
    // Check pretty-formatting of an enum declaration.
    let pretty = pretty_format_type(&vec![
        Token::new_atom("enum"),
        Token::new_atom("test"),
        Token::new_atom("{"),
        Token::new_atom("VALUE1"),
        Token::new_atom(","),
        Token::new_atom("VALUE2"),
        Token::new_atom(","),
        Token::new_atom("VALUE3"),
        Token::new_atom("}"),
    ]);
    assert_eq!(
        pretty,
        crate::string_vec!(
            "enum test {",
            "\tVALUE1,",
            "\tVALUE2,",
            "\tVALUE3",
            "}" //
        )
    );
}

#[test]
fn format_struct() {
    // Check pretty-formatting of a struct declaration.
    let pretty = pretty_format_type(&vec![
        Token::new_atom("struct"),
        Token::new_atom("test"),
        Token::new_atom("{"),
        Token::new_atom("int"),
        Token::new_atom("ivalue"),
        Token::new_atom(";"),
        Token::new_atom("long"),
        Token::new_atom("lvalue"),
        Token::new_atom(";"),
        Token::new_atom("}"),
    ]);
    assert_eq!(
        pretty,
        crate::string_vec!(
            "struct test {",
            "\tint ivalue;",
            "\tlong lvalue;",
            "}" //
        )
    );
}

#[test]
fn format_union() {
    // Check pretty-formatting of a union declaration.
    let pretty = pretty_format_type(&vec![
        Token::new_atom("union"),
        Token::new_atom("test"),
        Token::new_atom("{"),
        Token::new_atom("int"),
        Token::new_atom("ivalue"),
        Token::new_atom(";"),
        Token::new_atom("long"),
        Token::new_atom("lvalue"),
        Token::new_atom(";"),
        Token::new_atom("}"),
    ]);
    assert_eq!(
        pretty,
        crate::string_vec!(
            "union test {",
            "\tint ivalue;",
            "\tlong lvalue;",
            "}" //
        )
    );
}

#[test]
fn format_enum_constant() {
    // Check pretty-formatting of an enum constant declaration.
    let pretty = pretty_format_type(&vec![Token::new_atom("7")]);
    assert_eq!(
        pretty,
        crate::string_vec!(
                "7" //
            )
    );
}

#[test]
fn format_nested() {
    // Check pretty-formatting of a nested declaration.
    let pretty = pretty_format_type(&vec![
        Token::new_atom("union"),
        Token::new_atom("nested"),
        Token::new_atom("{"),
        Token::new_atom("struct"),
        Token::new_atom("{"),
        Token::new_atom("int"),
        Token::new_atom("ivalue1"),
        Token::new_atom(";"),
        Token::new_atom("int"),
        Token::new_atom("ivalue2"),
        Token::new_atom(";"),
        Token::new_atom("}"),
        Token::new_atom(";"),
        Token::new_atom("long"),
        Token::new_atom("lvalue"),
        Token::new_atom(";"),
        Token::new_atom("}"),
    ]);
    assert_eq!(
        pretty,
        crate::string_vec!(
            "union nested {",
            "\tstruct {",
            "\t\tint ivalue1;",
            "\t\tint ivalue2;",
            "\t};",
            "\tlong lvalue;",
            "}" //
        )
    );
}

#[test]
fn format_imbalanced() {
    // Check pretty-formatting of a declaration with wrongly balanced brackets.
    let pretty = pretty_format_type(&vec![
        Token::new_atom("struct"),
        Token::new_atom("imbalanced"),
        Token::new_atom("{"),
        Token::new_atom("{"),
        Token::new_atom("}"),
        Token::new_atom("}"),
        Token::new_atom("}"),
        Token::new_atom(";"),
        Token::new_atom("{"),
        Token::new_atom("{"),
    ]);
    assert_eq!(
        pretty,
        crate::string_vec!(
            "struct imbalanced {",
            "\t{",
            "\t}",
            "}",
            "};",
            "{",
            "\t{" //
        )
    );
}

#[test]
fn format_typeref() {
    // Check pretty-formatting of a declaration with a reference to another type.
    let pretty = pretty_format_type(&vec![
        Token::new_atom("struct"),
        Token::new_atom("typeref"),
        Token::new_atom("{"),
        Token::new_typeref("s#other"),
        Token::new_atom("other"),
        Token::new_atom(";"),
        Token::new_atom("}"),
    ]);
    assert_eq!(
        pretty,
        crate::string_vec!(
            "struct typeref {",
            "\ts#other other;",
            "}" //
        )
    );
}

#[test]
fn format_removal() {
    // TODO Add test description.
    let diff = get_type_diff(
        &vec![
            Token::new_atom("struct"),
            Token::new_atom("test"),
            Token::new_atom("{"),
            Token::new_atom("int"),
            Token::new_atom("ivalue1"),
            Token::new_atom(";"),
            Token::new_atom("int"),
            Token::new_atom("ivalue2"),
            Token::new_atom(";"),
            Token::new_atom("}"),
        ],
        &vec![
            Token::new_atom("struct"),
            Token::new_atom("test"),
            Token::new_atom("{"),
            Token::new_atom("int"),
            Token::new_atom("ivalue1"),
            Token::new_atom(";"),
            Token::new_atom("}"),
        ],
    );
    assert_eq!(
        diff,
        crate::string_vec!(
            " struct test {",
            " \tint ivalue1;",
            "-\tint ivalue2;",
            " }" //
        )
    );
}

#[test]
fn format_addition() {
    // TODO Add test description.
    let diff = get_type_diff(
        &vec![
            Token::new_atom("struct"),
            Token::new_atom("test"),
            Token::new_atom("{"),
            Token::new_atom("int"),
            Token::new_atom("ivalue1"),
            Token::new_atom(";"),
            Token::new_atom("}"),
        ],
        &vec![
            Token::new_atom("struct"),
            Token::new_atom("test"),
            Token::new_atom("{"),
            Token::new_atom("int"),
            Token::new_atom("ivalue1"),
            Token::new_atom(";"),
            Token::new_atom("int"),
            Token::new_atom("ivalue2"),
            Token::new_atom(";"),
            Token::new_atom("}"),
        ],
    );
    assert_eq!(
        diff,
        crate::string_vec!(
            " struct test {",
            " \tint ivalue1;",
            "+\tint ivalue2;",
            " }" //
        )
    );
}

#[test]
fn format_modification() {
    // TODO Add test description.
    let diff = get_type_diff(
        &vec![
            Token::new_atom("struct"),
            Token::new_atom("test"),
            Token::new_atom("{"),
            Token::new_atom("int"),
            Token::new_atom("ivalue1"),
            Token::new_atom(";"),
            Token::new_atom("}"),
        ],
        &vec![
            Token::new_atom("struct"),
            Token::new_atom("test"),
            Token::new_atom("{"),
            Token::new_atom("int"),
            Token::new_atom("ivalue2"),
            Token::new_atom(";"),
            Token::new_atom("}"),
        ],
    );
    assert_eq!(
        diff,
        crate::string_vec!(
            " struct test {",
            "-\tint ivalue1;",
            "+\tint ivalue2;",
            " }" //
        )
    );
}
