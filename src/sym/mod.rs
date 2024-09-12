// Copyright (C) 2024 SUSE LLC <petr.pavlu@suse.com>
// SPDX-License-Identifier: GPL-2.0-or-later

use log::debug;
use std::cmp::min;
use std::collections::hash_map::Entry::{Occupied, Vacant};
use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{prelude::*, BufReader, BufWriter};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use std::{fs, io, thread};

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
type TypeVariants = Vec<Tokens>;
type Types = HashMap<String, TypeVariants>;
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

struct ParallelLoadContext {
    types: Mutex<Types>,
    exports: Mutex<Exports>,
    files: Mutex<SymFiles>,
}

impl SymCorpus {
    pub fn new() -> Self {
        Self {
            types: Types::new(),
            exports: Exports::new(),
            files: SymFiles::new(),
        }
    }

    // TODO Describe.
    pub fn load(&mut self, path: &Path, num_workers: i32) -> Result<(), crate::Error> {
        // Determine if the input is a directory tree or a single symtypes file.
        let md = match fs::metadata(path) {
            Ok(md) => md,
            Err(err) => {
                return Err(crate::Error::new_io(
                    &format!("Failed to query path '{}'", path.display()),
                    err,
                ))
            }
        };

        // Collect recursively all symtypes if it is a directory, or push the single file.
        let mut symfiles = Vec::new();
        if md.is_dir() {
            Self::collect_symfiles(path, &mut symfiles)?;
        } else {
            symfiles.push(path.to_path_buf());
        }

        // Load all files.
        self.load_multiple(&symfiles, num_workers)
    }

    /// Collects recursively all symtypes under a given path.
    fn collect_symfiles(path: &Path, symfiles: &mut Vec<PathBuf>) -> Result<(), crate::Error> {
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
                Self::collect_symfiles(&entry_path, symfiles)?;
                continue;
            }

            let file_name = entry.file_name();
            let ext = match Path::new(&file_name).extension() {
                Some(ext) => ext,
                None => continue,
            };
            if ext == "symtypes" {
                symfiles.push(entry_path.to_path_buf());
            }
        }
        Ok(())
    }

    /// Loads all specified symtypes.
    pub fn load_multiple(
        &mut self,
        symfiles: &Vec<PathBuf>,
        num_workers: i32,
    ) -> Result<(), crate::Error> {
        // Load data from the files.
        let next_work_idx = AtomicUsize::new(0);

        let load_context = ParallelLoadContext {
            types: Mutex::new(Types::new()),
            exports: Mutex::new(Exports::new()),
            files: Mutex::new(SymFiles::new()),
        };

        thread::scope(|s| {
            for _ in 0..num_workers {
                // TODO Result/Error handling.
                s.spawn(|| loop {
                    let work_idx = next_work_idx.fetch_add(1, Ordering::Relaxed);
                    if work_idx >= symfiles.len() {
                        return Ok(());
                    }
                    let path = symfiles[work_idx].as_path();

                    let file = match File::open(path) {
                        Ok(file) => file,
                        Err(err) => {
                            return Err(crate::Error::new_io(
                                &format!("Failed to open file '{}'", path.display()),
                                err,
                            ))
                        }
                    };

                    Self::load_single(path, file, &load_context)?;
                });
            }
        });

        *self = Self {
            types: load_context.types.into_inner().unwrap(),
            exports: load_context.exports.into_inner().unwrap(),
            files: load_context.files.into_inner().unwrap(),
        };

        Ok(())
    }

    /// Loads symtypes data from a specified reader.
    fn load_single<R>(
        path: &Path,
        reader: R,
        load_context: &ParallelLoadContext,
    ) -> Result<(), crate::Error>
    where
        R: io::Read,
    {
        debug!("Loading {}", path.display());

        // Read all declarations.
        // TODO Describe the types.
        let reader = BufReader::new(reader);
        let mut records = FileRecords::new();
        let mut remap = HashMap::new();

        // Read the file and split its content into a lines vector.
        let mut lines = Vec::new();
        for maybe_line in reader.lines() {
            match maybe_line {
                Ok(line) => lines.push(line),
                Err(err) => {
                    return Err(crate::Error::new_io(
                        &format!("Failed to read data from file '{}'", path.display()),
                        err,
                    ))
                }
            };
        }

        // Detect whether the input is a single or consolidated symtypes file.
        let mut is_consolidated = false;
        for line in &lines {
            if line.starts_with("F#") {
                is_consolidated = true;
                break;
            }
        }

        // Parse all declarations.
        let mut file_indices = Vec::new();
        for (i, line) in lines.iter().enumerate() {
            // Check for a file declaration and remember its index. The file declarations are
            // processed later after remapping of all symbol variants is known.
            if line.starts_with("F#") {
                file_indices.push(i);
                continue;
            }

            // Handle a type/export record.
            let mut words = line.split_ascii_whitespace();

            let mut name = match words.next() {
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

            // Parse the variant name/index which is appended as a suffix after the `@` character.
            let orig_variant_name;
            match name.rfind('@') {
                Some(i) => {
                    orig_variant_name = &name[i + 1..];
                    name = &name[..i];
                }
                None => {
                    orig_variant_name = "";
                }
            }

            // Insert the type into the corpus.
            let variant_idx = Self::merge_type(name, tokens, &load_context.types);

            // Record a mapping from the original variant name/index to the new one.
            if is_consolidated {
                remap
                    .entry(name.to_string())
                    .or_insert_with(|| HashMap::new())
                    .insert(orig_variant_name.to_string(), variant_idx);
            } else {
                // TODO What if a @variant suffix is found in non-consolidated file?
                records.insert(name.to_string(), variant_idx);

                // TODO Check for duplicates.
                if Self::is_export(name) {
                    let mut exports = load_context.exports.lock().unwrap();
                    // TODO FIXME Fix the race.
                    let mut files = load_context.files.lock().unwrap();
                    let file_idx = files.len();
                    exports.insert(name.to_string(), files.len());
                }
            }
        }

        // TODO Validate all references?

        if is_consolidated {
            // Handle file declarations.
            for i in file_indices {
                let mut words = lines[i].split_ascii_whitespace();

                let record_name = words.next().unwrap();
                assert!(record_name.starts_with("F#"));
                let file_name = &record_name[2..];

                let mut records = FileRecords::new();
                for mut type_name in words {
                    // Parse the variant name/index.
                    let orig_variant_name;
                    match type_name.rfind('@') {
                        Some(i) => {
                            orig_variant_name = &type_name[i + 1..];
                            type_name = &type_name[..i];
                        }
                        None => {
                            orig_variant_name = "";
                        }
                    }

                    // Look up how the variant got remapped.
                    // TODO De-duplicate error messages.
                    let variant_idx = match remap.get(type_name) {
                        Some(hash) => match hash.get(orig_variant_name) {
                            Some(&variant_idx) => variant_idx,
                            None => {
                                return Err(crate::Error::new_parse(&format!(
                                    "Type {}@{} is not known in file {}",
                                    type_name,
                                    orig_variant_name,
                                    path.display(),
                                )))
                            }
                        },
                        None => {
                            return Err(crate::Error::new_parse(&format!(
                                "Type {}@{} is not known in file {}",
                                type_name,
                                orig_variant_name,
                                path.display(),
                            )))
                        }
                    };
                    records.insert(type_name.to_string(), variant_idx);

                    // TODO Check for duplicates.
                    if Self::is_export(type_name) {
                        let mut exports = load_context.exports.lock().unwrap();
                        // TODO FIXME Fix the race.
                        let mut files = load_context.files.lock().unwrap();
                        let file_idx = files.len();
                        exports.insert(type_name.to_string(), file_idx);
                    }
                }

                // Add implicit references, ones that were omitted by the F# declaration because
                // only one variant exists in the entire consolidated file.
                let walk_records: Vec<_> = records
                    .iter()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect();
                for (name, variant_idx) in walk_records {
                    // TODO Simplify.
                    let types = load_context.types.lock().unwrap();
                    Self::extrapolate_file_record(
                        path,
                        file_name,
                        &name,
                        variant_idx,
                        true,
                        &*types,
                        &mut records,
                    );
                }

                let symfile = SymFile {
                    path: Path::new(file_name).to_path_buf(),
                    records: records,
                };
                let mut files = load_context.files.lock().unwrap();
                files.push(symfile);
            }
        } else {
            // TODO Drop the root prefix.
            let symfile = SymFile {
                path: path.to_path_buf(),
                records: records,
            };
            let mut files = load_context.files.lock().unwrap();
            files.push(symfile);
        }

        Ok(())
    }

    fn merge_type(name: &str, tokens: Tokens, types: &Mutex<Types>) -> usize {
        let mut types = types.lock().unwrap();
        // TODO Use .entry()?
        match types.get_mut(name) {
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
                types.insert(name.to_string(), variants);
                return 0;
            }
        }
    }

    /// Processes a single symbol in some file originated from an `F#` record and enhances the
    /// specified file records with the needed implicit types.
    ///
    /// This function is used when reading a consolidated input file and processing its `F#`
    /// records. Each `F#` record is in form `F#<filename> <type@variant>... <export>...`. It lists
    /// all types and exports in a given file but is allowed to omit any referenced types which have
    /// only one variant in the whole consolidated file. The purpose of this function is to find all
    /// such implicit references and add them to `records`.
    ///
    /// A caller of this function should pre-fill `records` with all explicit references given on
    /// the processed `F#` record and then call this function on each of the references. These root
    /// calls should be invoked with `is_explicit` set to `true`. The function then recursively adds
    /// all needed implicit types which are referenced from these roots.
    fn extrapolate_file_record(
        corpus_path: &Path,
        file_name: &str,
        name: &str,
        variant_idx: usize,
        is_explicit: bool,
        types: &Types,
        records: &mut FileRecords,
    ) -> Result<(), crate::Error> {
        if is_explicit {
            // All explicit symbols need to be added by the caller.
            assert!(records.get(name).is_some());
        } else {
            // A symbol can be implicit only if it has one variant.
            assert!(variant_idx == 0);

            // See if the symbol was already processed.
            //
            // Unfortunately, HashMap in stable Rust doesn't offer to do a lookup using &str but
            // insert the key as String if it is missing. The code opts to run the lookup again
            // if the key is missing and the key+value pair needs inserting.
            // https://stackoverflow.com/questions/51542024/how-do-i-use-the-entry-api-with-an-expensive-key-that-is-only-constructed-if-the
            if records.get(name).is_some() {
                return Ok(());
            }
            records.insert(name.to_string(), variant_idx);
        }

        // Obtain tokens for the selected variant and check it is correctly specified.
        let variants = types.get(name).unwrap();
        assert!(variants.len() > 0);
        if !is_explicit && variants.len() > 1 {
            return Err(crate::Error::new_parse(&format!(
                "Type '{}' is implicitly referenced by file '{}' but has multiple variants in corpus '{}'",
                name,
                file_name,
                corpus_path.display(),
            )));
        }
        let tokens = &variants[variant_idx];

        // Process recursively all types referenced by this symbol.
        for token in tokens {
            match token {
                Token::TypeRef(ref_name) => {
                    // Process the type. Note that passing variant_idx=0 is ok here:
                    // * If the type is explicitly specified in the parent F# record then it must be
                    //   already added in the records and the called function immediately returns.
                    // * If the type is implicit then it can have only one variant and so only
                    //   variant_idx=0 can be correct. The invoked function will check that no more
                    //   than one variant is actually present.
                    Self::extrapolate_file_record(
                        corpus_path,
                        file_name,
                        ref_name,
                        0,
                        false,
                        types,
                        records,
                    );
                }
                Token::Atom(_word) => {}
            }
        }

        Ok(())
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
    pub fn write_consolidated_file(&self, filename: &str) -> Result<(), crate::Error> {
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

        self.write_consolidated(file)
    }

    pub fn write_consolidated<W>(&self, writer: W) -> Result<(), crate::Error>
    where
        W: io::Write,
    {
        let mut writer = BufWriter::new(writer);

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
        sorted_records.sort_by_key(|(name, _remap)| (Self::is_export(name), *name));

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

            // Output the F# record in form `F#<filename> <type@variant>... <export>...`. Types with
            // only one variant in the entire consolidated file can be skipped because they can be
            // implicitly determined by a reader.
            write!(writer, "F#{}", symfile.path.display());
            for &(_, name, remap_idx) in &sorted_types {
                if remap_idx != usize::MAX {
                    write!(writer, " {}@{}", name, remap_idx);
                } else if Self::is_export(name) {
                    write!(writer, " {}", name);
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
        changes: &Mutex<TypeChanges<'a>>,
    ) {
        let mut changes = changes.lock().unwrap();
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
        changes: &Mutex<TypeChanges<'a>>,
    ) {
        // TODO Take into account different variants?
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

    pub fn compare_with(&self, other: &SymCorpus, num_workers: i32) {
        let works: Vec<_> = self.exports.iter().collect();
        let next_work_idx = AtomicUsize::new(0);

        let changes = Mutex::new(TypeChanges::new());

        thread::scope(|s| {
            for _ in 0..num_workers {
                s.spawn(|| loop {
                    let work_idx = next_work_idx.fetch_add(1, Ordering::Relaxed);
                    if work_idx >= works.len() {
                        break;
                    }
                    let (name, file_idx) = works[work_idx];

                    let file = &self.files[*file_idx];
                    match other.exports.get(name) {
                        Some(other_file_idx) => {
                            let other_file = &other.files[*other_file_idx];
                            let mut processed = HashSet::new();
                            self.compare_types(
                                other,
                                file,
                                other_file,
                                name,
                                &mut processed,
                                &changes,
                            );
                        }
                        None => {
                            println!("Export {} is present in A but not in B", name);
                        }
                    }
                });
            }
        });

        // Check for symbols in B and not in A.
        for (other_name, _other_file_idx) in &other.exports {
            match self.exports.get(other_name) {
                Some(_file_idx) => {}
                None => {
                    println!("Export {} is present in B but not in A", other_name);
                }
            }
        }

        let changes = changes.into_inner().unwrap();
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
