// Copyright (C) 2024 SUSE LLC <petr.pavlu@suse.com>
// SPDX-License-Identifier: GPL-2.0-or-later

use log::debug;
use std::cmp::min;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::fs::File;
use std::io::{prelude::*, BufReader};
use std::path::{Path, PathBuf};

#[derive(Eq, PartialEq)]
enum Token {
    TypeRef(String),
    Atom(String),
}

impl Token {
    fn as_str(&self) -> &str {
        match self {
            Self::TypeRef(ref_name) => ref_name.as_str(),
            Self::Atom(word) => word.as_str(),
        }
    }
}

type Tokens = Vec<Token>;
type Types = HashMap<String, Vec<Tokens>>;
type Exports = HashMap<String, usize>;
type FileRecords = HashMap<String, usize>;

struct SymFile {
    path: PathBuf,
    records: FileRecords,
}

type SymFiles = Vec<SymFile>;

pub struct SymCorpus {
    types: Types,
    exports: Exports,
    files: SymFiles,
}

type TypeChanges<'a> = HashMap<&'a str, Vec<(&'a Tokens, &'a Tokens)>>;

impl SymCorpus {
    pub fn new(dir: &str) -> Result<Self, crate::Error> {
        let mut symtypes = Self {
            types: Types::new(),
            exports: Exports::new(),
            files: SymFiles::new(),
        };
        symtypes.load_dir(&Path::new(dir))?;
        Ok(symtypes)
    }

    /// Loads symtypes in a specified directory, recursively.
    fn load_dir(&mut self, path: &Path) -> Result<(), crate::Error> {
        // TODO Report errors and skip directories?
        let dir_iter = match fs::read_dir(path) {
            Ok(dir_iter) => dir_iter,
            Err(err) => {
                return Err(crate::Error::new_io(
                    &format!("Failed to read directory '{}'", path.display()),
                    err,
                ))
            }
        };
        for maybe_entry in dir_iter {
            let entry = match maybe_entry {
                Ok(entry) => entry,
                Err(err) => {
                    return Err(crate::Error::new_io(
                        &format!("Failed to read directory '{}'", path.display()),
                        err,
                    ))
                }
            };
            let entry_path = entry.path();
            if entry_path.is_dir() {
                self.load_dir(&entry_path)?;
                continue;
            }

            let file_name = entry.file_name();
            let ext = match Path::new(&file_name).extension() {
                Some(ext) => ext,
                None => continue,
            };
            if ext == "symtypes" {
                self.load_file(&entry_path)?;
            }
        }
        Ok(())
    }

    /// Loads symtypes data from a specified file.
    fn load_file(&mut self, path: &Path) -> Result<(), crate::Error> {
        debug!("Loading {}", path.display());

        let file = match File::open(path) {
            Ok(file) => file,
            Err(err) => {
                return Err(crate::Error::new_io(
                    &format!("Failed to open file '{}'", path.display()),
                    err,
                ))
            }
        };
        let reader = BufReader::new(file);

        // Read all declarations.
        let mut records = FileRecords::new();

        for maybe_line in reader.lines() {
            let line = match maybe_line {
                Ok(line) => line,
                Err(err) => {
                    return Err(crate::Error::new_io(
                        &format!("Failed to read data from file '{}'", path.display()),
                        err,
                    ))
                }
            };
            let mut words = line.split_ascii_whitespace();

            let name = match words.next() {
                Some(word) => word,
                None => continue, // TODO
            };

            let mut tokens = Vec::new();
            for word in words {
                let mut is_typeref = false;
                match word.chars().nth(1) {
                    Some(ch) => {
                        if ch == '#' {
                            is_typeref = true;
                        }
                    }
                    None => {}
                }
                tokens.push(if is_typeref {
                    Token::TypeRef(word.to_string())
                } else {
                    Token::Atom(word.to_string())
                });
            }

            let index = self.merge_type(name, tokens);
            records.insert(name.to_string(), index);

            // TODO Check for duplicates.
            match name.chars().nth(1) {
                Some(ch) => {
                    if ch != '#' {
                        self.exports.insert(name.to_string(), self.files.len());
                    }
                }
                None => {}
            }
        }

        // TODO Validate all references?

        let symfile = SymFile {
            path: path.to_path_buf(),
            records: records,
        };
        self.files.push(symfile);

        Ok(())
    }

    fn merge_type(&mut self, name: &str, tokens: Tokens) -> usize {
        match self.types.get_mut(name) {
            Some(variants) => {
                for (i, variant) in variants.iter().enumerate() {
                    if Self::are_tokens_eq(&tokens, variant) {
                        return i;
                    }
                }
                variants.push(tokens);
                return variants.len() - 1;
            }
            None => {
                let mut variants = Vec::new();
                variants.push(tokens);
                self.types.insert(name.to_string(), variants);
                return 0;
            }
        }
    }

    fn are_tokens_eq(a: &Tokens, b: &Tokens) -> bool {
        if a.len() != b.len() {
            return false;
        }
        for i in 0..a.len() {
            if a[i] != b[i] {
                return false;
            };
        }
        return true;
    }

    // TODO
    fn print_file_type(&self, file: &SymFile, name: &str, processed: &mut HashSet<String>) {
        match processed.get(name) {
            Some(_) => return,
            None => {}
        }
        processed.insert(name.to_string());

        match file.records.get(name) {
            Some(variant_idx) => match self.types.get(name) {
                Some(variants) => {
                    let tokens = &variants[*variant_idx];
                    for token in tokens.iter() {
                        match token {
                            Token::TypeRef(ref_name) => {
                                self.print_file_type(file, ref_name, processed);
                            }
                            Token::Atom(_word) => {}
                        }
                    }

                    print!("{}", name);
                    for token in tokens.iter() {
                        match token {
                            Token::TypeRef(ref_name) => {
                                print!(" {}", ref_name);
                            }
                            Token::Atom(word) => {
                                print!(" {}", word);
                            }
                        }
                    }
                    println!("");
                }
                None => {
                    panic!("Type {} has a missing declaration", name);
                }
            },
            None => {
                panic!("Type {} is not known in file {}", name, file.path.display())
            }
        }
    }

    pub fn print_type(&self, name: &str) {
        for file in self.files.iter() {
            match file.records.get(name) {
                Some(_variant_idx) => {
                    println!("Found type {} in {}:", name, file.path.display());
                    let mut processed = HashSet::new();
                    self.print_file_type(&file, name, &mut processed);
                }
                None => {}
            }
        }
    }

    fn get_type_tokens<'a>(symtypes: &'a SymCorpus, file: &SymFile, name: &str) -> &'a Tokens {
        match file.records.get(name) {
            Some(variant_idx) => match symtypes.types.get(name) {
                Some(variants) => &variants[*variant_idx],
                None => {
                    panic!("Type {} has a missing declaration", name);
                }
            },
            None => {
                panic!("Type {} is not known in file {}", name, file.path.display())
            }
        }
    }

    fn record_type_change<'a>(
        name: &'a str,
        tokens: &'a Tokens,
        other_tokens: &'a Tokens,
        changes: &mut TypeChanges<'a>,
    ) {
        match changes.get_mut(name) {
            Some(variants) => {
                for (tokens2, other_tokens2) in variants.iter() {
                    if Self::are_tokens_eq(tokens, tokens2)
                        && Self::are_tokens_eq(other_tokens, other_tokens2)
                    {
                        return;
                    }
                }
                variants.push((tokens, other_tokens));
            }
            None => {
                let mut variants = Vec::new();
                variants.push((tokens, other_tokens));
                changes.insert(name, variants);
            }
        }
    }

    fn compare_types<'a>(
        &'a self,
        other: &'a SymCorpus,
        file: &SymFile,
        other_file: &SymFile,
        name: &'a str,
        processed: &mut HashSet<String>,
        changes: &mut TypeChanges<'a>,
    ) {
        match processed.get(name) {
            Some(_) => return,
            None => {}
        }
        processed.insert(name.to_string());

        let tokens = Self::get_type_tokens(self, file, name);
        let other_tokens = Self::get_type_tokens(other, other_file, name);

        let mut is_equal = tokens.len() == other_tokens.len();
        let min_tokens = min(tokens.len(), other_tokens.len());
        for i in 0..min_tokens {
            let token = &tokens[i];
            let other_token = &other_tokens[i];

            is_equal &= match (token, other_token) {
                (Token::TypeRef(ref_name), Token::TypeRef(other_ref_name)) => {
                    if ref_name == other_ref_name {
                        self.compare_types(
                            other,
                            file,
                            other_file,
                            ref_name.as_str(),
                            processed,
                            changes,
                        );
                        true
                    } else {
                        false
                    }
                }
                (Token::Atom(word), Token::Atom(other_word)) => word == other_word,
                _ => false,
            };
        }
        if !is_equal {
            // TODO
            Self::record_type_change(name, tokens, other_tokens, changes);
        }
    }

    fn pretty_format_type(name: &str, tokens: &Tokens) -> Vec<String> {
        let mut res = Vec::new();
        let mut indent = 0;

        let mut line = name.to_string();
        for token in tokens.iter() {
            let mut newline = false;
            match token.as_str() {
                "{" => {
                    line.push_str(" {");
                    res.push(line);
                    line = String::new();
                    indent += 1;
                    newline = true;
                }
                "}" => {
                    line.push_str(" }");
                    res.push(line);
                    line = String::new();
                    indent -= 1;
                    newline = true;
                }
                ";" => {
                    line.push_str(" ;");
                    res.push(line);
                    line = String::new();
                    newline = true;
                }
                "," => {
                    line.push_str(" ,");
                    res.push(line);
                    line = String::new();
                    newline = true;
                }
                _ => {
                    line.push(' ');
                    line.push_str(token.as_str());
                }
            };
            if newline {
                for _ in 0..indent {
                    line.push_str("\t");
                }
            }
        }
        res
    }

    fn print_type_change(name: &str, tokens: &Tokens, other_tokens: &Tokens) {
        println!("{}", name);
        let pretty = Self::pretty_format_type(name, tokens);
        let other_pretty = Self::pretty_format_type(name, other_tokens);

        let diff_output = crate::diff::unified(&pretty, &other_pretty);
        for line in diff_output.iter() {
            println!("{}", line);
        }
    }

    pub fn compare_with(&self, other: &SymCorpus) {
        let mut changes = TypeChanges::new();

        for (name, file_idx) in self.exports.iter() {
            let file = &self.files[*file_idx];
            match other.exports.get(name) {
                Some(other_file_idx) => {
                    let other_file = &other.files[*other_file_idx];
                    let mut processed = HashSet::new();
                    self.compare_types(other, file, other_file, name, &mut processed, &mut changes);
                }
                None => {
                    println!("Export {} is present in A but not in B", name);
                }
            }
        }

        // Check for symbols in B and not in A.
        for (other_name, _other_file_idx) in other.exports.iter() {
            match self.exports.get(other_name) {
                Some(_file_idx) => {}
                None => {
                    println!("Export {} is present in B but not in A", other_name);
                }
            }
        }

        for (name, variants) in changes.iter() {
            for (tokens, other_tokens) in variants {
                Self::print_type_change(name, tokens, other_tokens);
            }
        }
    }
}
