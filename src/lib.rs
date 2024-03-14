// Copyright (C) 2024 SUSE LLC <petr.pavlu@suse.com>
// SPDX-License-Identifier: GPL-2.0-or-later

use log::debug;
use std::cmp::min;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::fs::File;
use std::io::{prelude::*, BufReader};
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub enum Error {
    IO {
        desc: String,
        io_err: std::io::Error,
    },
    Parse(String),
}

impl Error {
    fn new_io(desc: &str, io_err: std::io::Error) -> Self {
        Error::IO {
            desc: desc.to_string(),
            io_err: io_err,
        }
    }

    fn new_parse(desc: &str) -> Self {
        Error::Parse(desc.to_string())
    }
}

impl std::error::Error for Error {}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Self::IO { desc, io_err } => {
                write!(f, "{}: ", desc)?;
                io_err.fmt(f)
            }
            Self::Parse(desc) => write!(f, "{}", desc),
        }
    }
}

#[derive(Eq, PartialEq)]
enum Token {
    TypeRef(String),
    Atom(String),
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

// TODO Rename to SymCorpus.
pub struct SymTypes {
    types: Types,
    exports: Exports,
    files: SymFiles,
}

impl SymTypes {
    pub fn new(dir: &str) -> Result<Self, Error> {
        let mut symtypes = Self {
            types: Types::new(),
            exports: Exports::new(),
            files: SymFiles::new(),
        };
        symtypes.load_dir(&Path::new(dir))?;
        Ok(symtypes)
    }

    /// Loads symtypes in a specified directory, recursively.
    fn load_dir(&mut self, path: &Path) -> Result<(), Error> {
        // TODO Report errors and skip directories?
        let dir_iter = match fs::read_dir(path) {
            Ok(dir_iter) => dir_iter,
            Err(err) => {
                return Err(Error::new_io(
                    &format!("Failed to read directory '{}'", path.display()),
                    err,
                ))
            }
        };
        for maybe_entry in dir_iter {
            let entry = match maybe_entry {
                Ok(entry) => entry,
                Err(err) => {
                    return Err(Error::new_io(
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
    fn load_file(&mut self, path: &Path) -> Result<(), Error> {
        debug!("Loading {}", path.display());

        let file = match File::open(path) {
            Ok(file) => file,
            Err(err) => {
                return Err(Error::new_io(
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
                    return Err(Error::new_io(
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

    fn get_type_tokens<'a>(symtypes: &'a SymTypes, file: &SymFile, name: &str) -> &'a Tokens {
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

    fn compare_types(
        &self,
        other: &SymTypes,
        file: &SymFile,
        other_file: &SymFile,
        name: &str,
        processed: &mut HashSet<String>,
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
                        self.compare_types(other, file, other_file, ref_name.as_str(), processed);
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
            println!("Type {} is not equal", name);
        }
    }

    pub fn compare_with(&self, other: &SymTypes) {
        for (name, file_idx) in self.exports.iter() {
            let file = &self.files[*file_idx];
            match other.exports.get(name) {
                Some(other_file_idx) => {
                    let other_file = &other.files[*other_file_idx];
                    let mut processed = HashSet::new();
                    self.compare_types(other, file, other_file, name, &mut processed);
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
    }
}
