// Copyright (C) 2024 SUSE LLC <petr.pavlu@suse.com>
// SPDX-License-Identifier: GPL-2.0-or-later

pub struct UniDiff<'a> {
    old: &'a Vec<String>,
    new: &'a Vec<String>,
    output: Vec<String>,
}

impl UniDiff<'_> {
    fn push_output(&mut self, prefix: char, lines: &[String]) {
        for line in lines.iter() {
            let mut marked_line = String::new();
            marked_line.push(prefix);
            marked_line.push_str(line);
            self.output.push(marked_line);
        }
    }
}

// TODO
impl diffs::Diff for UniDiff<'_> {
    type Error = crate::Error;

    fn equal(&mut self, old: usize, _new: usize, len: usize) -> Result<(), Self::Error> {
        self.push_output(' ', &self.old[old..old + len]);
        Ok(())
    }

    fn delete(&mut self, old: usize, len: usize, _new: usize) -> Result<(), Self::Error> {
        self.push_output('-', &self.old[old..old + len]);
        Ok(())
    }

    fn insert(&mut self, _old: usize, new: usize, new_len: usize) -> Result<(), Self::Error> {
        self.push_output('+', &self.new[new..new + new_len]);
        Ok(())
    }

    fn replace(
        &mut self,
        old: usize,
        old_len: usize,
        new: usize,
        new_len: usize,
    ) -> Result<(), Self::Error> {
        self.push_output('-', &self.old[old..old + old_len]);
        self.push_output('+', &self.new[new..new + new_len]);
        Ok(())
    }

    fn finish(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }
}

pub fn unified(old: &Vec<String>, new: &Vec<String>) -> Vec<String> {
    let mut diff = UniDiff {
        old: old,
        new: new,
        output: Vec::new(),
    };
    // TODO Check the result.
    diffs::myers::diff(&mut diff, old, 0, old.len(), new, 0, new.len());
    diff.output
}
