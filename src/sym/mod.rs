// Copyright (C) 2024 SUSE LLC <petr.pavlu@suse.com>
// SPDX-License-Identifier: GPL-2.0-or-later

use log::debug;
use std::cmp::min;
use std::collections::hash_map::Entry::{Occupied, Vacant};
use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{prelude::*, BufReader, BufWriter};
use std::path::{Path, PathBuf};
use std::{fs, io};

#[cfg(test)]
mod tests;

#[derive(Eq, PartialEq)]
enum Token {
    TypeRef(String),
    Atom(String),
}

impl Token {
    #[cfg(test)]
    fn new_typeref<S: Into<String>>(name: S) -> Self {
        Token::TypeRef(name.into())
    }

    #[cfg(test)]
    fn new_atom<S: Into<String>>(name: S) -> Self {
        Token::Atom(name.into())
    }

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
    pub fn new() -> Self {
        Self {
            types: Types::new(),
            exports: Exports::new(),
            files: SymFiles::new(),
        }
    }

    /// Loads symtypes in a specified directory, recursively.
    pub fn load_dir(&mut self, path: &Path) -> Result<(), crate::Error> {
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
                self.read_single_file(&entry_path)?;
            }
        }
        Ok(())
    }

    /// Loads symtypes data from a specified file.
    pub fn read_single_file(&mut self, path: &Path) -> Result<(), crate::Error> {
        let file = match File::open(path) {
            Ok(file) => file,
            Err(err) => {
                return Err(crate::Error::new_io(
                    &format!("Failed to open file '{}'", path.display()),
                    err,
                ))
            }
        };

        self.read_single(path, file)
    }

    /// Loads symtypes data from a specified reader.
    pub fn read_single<R>(&mut self, path: &Path, reader: R) -> Result<(), crate::Error>
        where R: io::Read,
    {
        debug!("Loading {}", path.display());

        // Read all declarations.
        let reader = BufReader::new(reader);
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
            if Self::is_export(name) {
                self.exports.insert(name.to_string(), self.files.len());
            }
        }

        // TODO Validate all references?

        // TODO Drop the root prefix.
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

    fn is_export(name: &str) -> bool {
        match name.chars().nth(1) {
            Some(ch) => ch != '#',
            None => true,
        }
    }

    /// Processes a single symbol specified in a given file and adds it to the consolidated output.
    ///
    /// The specified symbol is added to `output_types` and `processed_types`, if not already
    /// present, and all its type references get recursively processed in the same way. The
    /// `output_types` is a [`HashMap`] which records all symbols which should appear on the output
    /// together with a mapping from the internal symbol variant index to the output variant index.
    /// The `processed_types` is a [`HashMap`] which tracks all symbols in the current file and
    /// their output variant indices.
    fn consolidate_type<'a>(
        &'a self,
        symfile: &SymFile,
        name: &'a str,
        output_types: &mut HashMap<&'a str, HashMap<usize, usize>>,
        processed_types: &mut HashMap<&'a str, usize>,
    ) {
        // See if the symbol was already processed.
        let processed_entry = match processed_types.entry(name) {
            Occupied(_) => return,
            Vacant(processed_entry) => processed_entry,
        };

        // Look up the internal variant index.
        let variant_idx = match symfile.records.get(name) {
            Some(&variant_idx) => variant_idx,
            None => panic!(
                "Type {} is not known in file {}",
                name,
                symfile.path.display()
            ),
        };

        // Determine the output variant index for the symbol.
        let remap_idx;
        match output_types.entry(name) {
            Occupied(mut active_entry) => {
                let remap = active_entry.get_mut();
                let remap_len = remap.len();
                match remap.entry(variant_idx) {
                    Occupied(remap_entry) => {
                        remap_idx = *remap_entry.get();
                    }
                    Vacant(remap_entry) => {
                        remap_idx = remap_len;
                        remap_entry.insert(remap_idx);
                    }
                }
            }
            Vacant(active_entry) => {
                remap_idx = 0;
                active_entry.insert(HashMap::from([(variant_idx, remap_idx)]));
            }
        };
        processed_entry.insert(remap_idx);

        // Process recursively all types that the symbol references.
        let variants = match self.types.get(name) {
            Some(variants) => variants,
            None => panic!("Type {} has a missing declaration", name),
        };

        for token in &variants[variant_idx] {
            match token {
                Token::TypeRef(ref_name) => {
                    self.consolidate_type(symfile, ref_name, output_types, processed_types)
                }
                Token::Atom(_word) => {}
            }
        }
    }

    /// Writes the corpus in the consolidated form into a specified file.
    pub fn write_consolidated(&self, filename: &str) -> Result<(), crate::Error> {
        // Open the output file.
        let path = Path::new(filename);
        let file: Box<dyn Write> = if filename == "-" {
            Box::new(io::stdout())
        } else {
            match File::create(path) {
                Ok(file) => Box::new(file),
                Err(err) => {
                    return Err(crate::Error::new_io(
                        &format!("Failed to create file '{}'", path.display()),
                        err,
                    ))
                }
            }
        };
        let mut writer = BufWriter::new(file);

        // Initialize output data. Variable output_types records all output symbols, file_types
        // provides per-file information.
        let mut output_types = HashMap::new();
        let mut file_types = vec![HashMap::new(); self.files.len()];

        // Sort all files in the corpus by their path.
        let mut file_indices = (0..self.files.len()).collect::<Vec<_>>();
        file_indices.sort_by_key(|&i| &self.files[i].path);

        // Process the sorted files and add their needed types to the output.
        for &i in &file_indices {
            let symfile = &self.files[i];

            // Collect sorted exports in the file which are the roots for consolidation.
            let mut exports = Vec::new();
            for (name, _) in &symfile.records {
                if Self::is_export(name) {
                    exports.push(name.as_str());
                }
            }
            exports.sort();

            // Add the exported types and their needed types to the output.
            let mut processed_types = HashMap::new();
            for name in &exports {
                self.consolidate_type(symfile, name, &mut output_types, &mut processed_types);
            }
            file_types[i] = processed_types;
        }

        // Go through all files and their output types. Check if a given type has only one variant
        // in the output and mark it as such.
        for i in 0..file_types.len() {
            for (name, remap_idx) in &mut file_types[i] {
                let remap = output_types.get(name).unwrap();
                if remap.len() == 1 {
                    *remap_idx = usize::MAX;
                }
            }
        }

        // Sort all output types and write them to the specified file.
        let mut sorted_records = output_types.into_iter().collect::<Vec<_>>();
        sorted_records
            .sort_by_key(|(name, _remap)| (Self::is_export(name), *name));

        for (name, remap) in sorted_records {
            let variants = self.types.get(name).unwrap();
            let mut sorted_remap = remap
                .iter()
                .map(|(&variant_idx, &remap_idx)| (remap_idx, variant_idx))
                .collect::<Vec<_>>();
            sorted_remap.sort();

            let needs_suffix = sorted_remap.len() > 1;
            for (remap_idx, variant_idx) in sorted_remap {
                let tokens = &variants[variant_idx];

                if needs_suffix {
                    write!(writer, "{}@{}", name, remap_idx);
                } else {
                    write!(writer, "{}", name);
                }
                for token in tokens {
                    write!(writer, " {}", token.as_str());
                }
                writeln!(writer, "");
            }
        }

        // Write file records.
        for &i in &file_indices {
            let symfile = &self.files[i];

            // TODO Sorting, make same as above.
            let mut sorted_types = file_types[i]
                .iter()
                .map(|(&name, &remap_idx)| (Self::is_export(name), name, remap_idx))
                .collect::<Vec<_>>();
            sorted_types.sort();

            write!(writer, "F#{}", symfile.path.display());
            for &(_, name, remap_idx) in &sorted_types {
                if remap_idx == usize::MAX {
                    write!(writer, " {}", name);
                } else {
                    write!(writer, " {}@{}", name, remap_idx);
                }
            }
            writeln!(writer, "");
        }
        Ok(())
    }

    // TODO
    fn print_file_type(&self, file: &SymFile, name: &str, processed: &mut HashSet<String>) {
        match processed.get(name) {
            Some(_) => return,
            None => {}
        }
        processed.insert(name.to_string());

        match file.records.get(name) {
            Some(&variant_idx) => match self.types.get(name) {
                Some(variants) => {
                    let tokens = &variants[variant_idx];
                    for token in tokens {
                        match token {
                            Token::TypeRef(ref_name) => {
                                self.print_file_type(file, ref_name, processed);
                            }
                            Token::Atom(_word) => {}
                        }
                    }

                    print!("{}", name);
                    for token in tokens {
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
        for file in &self.files {
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
            Some(&variant_idx) => match symtypes.types.get(name) {
                Some(variants) => &variants[variant_idx],
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
        // TODO Rewrite using .entry().
        match changes.get_mut(name) {
            Some(variants) => {
                for (tokens2, other_tokens2) in &*variants {
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

    pub fn compare_with(&self, other: &SymCorpus) {
        let mut changes = TypeChanges::new();

        for (name, file_idx) in &self.exports {
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
        for (other_name, _other_file_idx) in &other.exports {
            match self.exports.get(other_name) {
                Some(_file_idx) => {}
                None => {
                    println!("Export {} is present in B but not in A", other_name);
                }
            }
        }

        for (name, variants) in changes {
            for (tokens, other_tokens) in variants {
                println!("{}", name);
                for line in get_type_diff(tokens, other_tokens) {
                    println!("{}", line);
                }
            }
        }
    }
}

/// Processes tokens describing a type and produces its pretty-formatted version as a [`Vec`] of
/// [`String`] lines.
fn pretty_format_type(tokens: &Tokens) -> Vec<String> {
    // Define a helper extension trait to allow appending a specific indentation to a string, as
    // string.push_indent().
    trait PushIndentExt {
        fn push_indent(&mut self, indent: usize);
    }

    impl PushIndentExt for String {
        fn push_indent(&mut self, indent: usize) {
            for _ in 0..indent {
                self.push_str("\t");
            }
        }
    }

    // Iterate over all tokens and produce the formatted output.
    let mut res = Vec::new();
    let mut indent = 0;

    let mut line = String::new();
    for token in tokens {
        // Handle the closing bracket early, it ends any prior line and reduces indentation.
        match token.as_str() {
            "}" => {
                if !line.is_empty() {
                    res.push(line);
                }
                if indent > 0 {
                    indent -= 1;
                }
                line = String::new();
            }
            _ => {}
        }

        // Insert any newline indentation.
        let is_first = line.is_empty();
        if is_first {
            line.push_indent(indent);
        }

        // Check if the token is special and append it appropriately to the output.
        match token.as_str() {
            "{" => {
                if !is_first {
                    line.push(' ');
                }
                line.push('{');
                res.push(line);
                indent += 1;

                line = String::new();
            }
            "}" => {
                line.push('}');
            }
            ";" => {
                line.push(';');
                res.push(line);

                line = String::new();
            }
            "," => {
                line.push(',');
                res.push(line);

                line = String::new();
            }
            _ => {
                if !is_first {
                    line.push(' ');
                }
                line.push_str(token.as_str());
            }
        };
    }

    if !line.is_empty() {
        res.push(line);
    }

    res
}

/// Formats a unified diff between two supposedly different types and returns them as a [`Vec`] of
/// [`String`] lines.
fn get_type_diff(tokens: &Tokens, other_tokens: &Tokens) -> Vec<String> {
    let pretty = pretty_format_type(tokens);
    let other_pretty = pretty_format_type(other_tokens);
    crate::diff::unified(&pretty, &other_pretty)
}
