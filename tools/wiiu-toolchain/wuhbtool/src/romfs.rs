/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! The RomFS archive of a WUHB bundle, laid out like devkitPro's wuhbtool writes it: a big endian
//! header with the `WUHB` magic, the file data from 0x200 and then the directory and file tables,
//! which a reader searches through their hash tables.

/// A file or a directory in the archive.
#[derive(Debug)]
pub(crate) enum Node {
    Directory { name: String, children: Vec<Node> },
    File { name: String, data: Vec<u8> },
}

const HEADER_SIZE: u32 = 0x50;
const FILE_PARTITION_OFFSET: usize = 0x200;
const DIRECTORY_ENTRY_SIZE: u32 = 0x18;
const FILE_ENTRY_SIZE: u32 = 0x20;
const ENTRY_EMPTY: u32 = 0xffff_ffff;

/// The number of hash table buckets for a number of entries, the next number without small prime
/// factors.
fn hash_table_count(entries: usize) -> usize {
    match entries {
        0..3 => 3,
        3..19 => entries | 1,
        _ => (entries..)
            .find(|count| {
                [2, 3, 5, 7, 11, 13, 17]
                    .iter()
                    .all(|prime| count % prime != 0)
            })
            .expect("a count without small prime factors"),
    }
}

/// The hash of a name in its parent directory, case insensitive.
pub(crate) fn path_hash(parent: u32, name: &str) -> u32 {
    name.bytes().fold(parent ^ 123_456_789, |hash, byte| {
        hash.rotate_right(5) ^ u32::from(byte.to_ascii_uppercase())
    })
}

const fn entry_size(base: u32, name: &str) -> u32 {
    base + (name.len() as u32).next_multiple_of(4)
}

enum Child {
    Directory(usize),
    File(usize),
}

struct Directory<'a> {
    name: &'a str,
    parent: usize,
    children: Vec<Child>,
    entry: u32,
}

struct File<'a> {
    name: &'a str,
    data: &'a [u8],
    entry: u32,
    offset: usize,
}

/// The tree flattened into tables, with the directory at index 0 as the root.
struct Tables<'a> {
    directories: Vec<Directory<'a>>,
    files: Vec<File<'a>>,
}

impl<'a> Tables<'a> {
    fn add(&mut self, parent: usize, nodes: &'a [Node]) {
        for node in nodes {
            match node {
                Node::Directory { name, children } => {
                    let index = self.directories.len();
                    self.directories.push(Directory {
                        name,
                        parent,
                        children: Vec::new(),
                        entry: 0,
                    });
                    self.directories[parent]
                        .children
                        .push(Child::Directory(index));
                    self.add(index, children);
                }
                Node::File { name, data } => {
                    self.directories[parent]
                        .children
                        .push(Child::File(self.files.len()));
                    self.files.push(File {
                        name,
                        data,
                        entry: 0,
                        offset: 0,
                    });
                }
            }
        }
    }

    fn child_directories(&self, directory: usize) -> impl Iterator<Item = usize> + '_ {
        self.directories[directory]
            .children
            .iter()
            .filter_map(|child| match child {
                Child::Directory(index) => Some(*index),
                Child::File(_) => None,
            })
    }

    fn child_files(&self, directory: usize) -> impl Iterator<Item = usize> + '_ {
        self.directories[directory]
            .children
            .iter()
            .filter_map(|child| match child {
                Child::File(index) => Some(*index),
                Child::Directory(_) => None,
            })
    }

    /// Numbers the directory entries depth first, each directory before its subdirectories.
    fn number_directories(&mut self, directory: usize, next: &mut u32) {
        let children: Vec<_> = self.child_directories(directory).collect();
        for child in children {
            self.directories[child].entry = *next;
            *next += entry_size(DIRECTORY_ENTRY_SIZE, self.directories[child].name);
            self.number_directories(child, next);
        }
    }

    /// Numbers the file entries and places their data, the files of subdirectories first.
    fn number_files(&mut self, directory: usize, next: &mut u32, data_size: &mut usize) {
        let children: Vec<_> = self.child_directories(directory).collect();
        for child in children {
            self.number_files(child, next, data_size);
        }
        let files: Vec<_> = self.child_files(directory).collect();
        for file in files {
            let file = &mut self.files[file];
            file.entry = *next;
            *next += entry_size(FILE_ENTRY_SIZE, file.name);
            file.offset = data_size.next_multiple_of(0x10);
            *data_size = file.offset + file.data.len();
        }
    }

    fn directory_entry(&self, child: &Child) -> Option<u32> {
        match child {
            Child::Directory(index) => Some(self.directories[*index].entry),
            Child::File(_) => None,
        }
    }

    fn file_entry(&self, child: &Child) -> Option<u32> {
        match child {
            Child::File(index) => Some(self.files[*index].entry),
            Child::Directory(_) => None,
        }
    }

    /// The entry of the first child of a kind, or of the next sibling of that kind after
    /// `position` in the children of a directory.
    fn next_entry(
        &self,
        directory: usize,
        position: Option<usize>,
        entry: impl Fn(&Self, &Child) -> Option<u32>,
    ) -> u32 {
        self.directories[directory]
            .children
            .iter()
            .skip(position.map_or(0, |position| position + 1))
            .find_map(|child| entry(self, child))
            .unwrap_or(ENTRY_EMPTY)
    }
}

struct HashedTable {
    hashes: Vec<u32>,
    table: Vec<u8>,
}

impl HashedTable {
    fn new(entries: usize, size: u32) -> Self {
        Self {
            hashes: vec![ENTRY_EMPTY; hash_table_count(entries)],
            table: vec![0; size as usize],
        }
    }

    /// Writes an entry with its fields, its hash chain link and its name, and makes it the first
    /// entry of its hash bucket.
    fn write(&mut self, entry: u32, fields: &[u8], hash: u32, name: &str) {
        let bucket = hash as usize % self.hashes.len();
        let link = self.hashes[bucket].to_be_bytes();
        let name_size = (name.len() as u32).to_be_bytes();
        let mut offset = entry as usize;
        for bytes in [fields, &link, &name_size, name.as_bytes()] {
            self.table[offset..offset + bytes.len()].copy_from_slice(bytes);
            offset += bytes.len();
        }
        self.hashes[bucket] = entry;
    }

    fn hashes_bytes(&self) -> impl Iterator<Item = u8> + '_ {
        self.hashes.iter().flat_map(|hash| hash.to_be_bytes())
    }
}

/// Writes the entries depth first like wuhbtool, the order decides the hash chains.
fn populate(
    tables: &Tables<'_>,
    directories: &mut HashedTable,
    files: &mut HashedTable,
    index: usize,
) {
    let directory = &tables.directories[index];
    let parent = tables.directories[directory.parent].entry;
    let position = (index > 0).then(|| {
        tables.directories[directory.parent]
            .children
            .iter()
            .position(|child| matches!(child, Child::Directory(child) if *child == index))
            .expect("a directory is a child of its parent")
    });
    let sibling = if index == 0 {
        ENTRY_EMPTY
    } else {
        tables.next_entry(directory.parent, position, Tables::directory_entry)
    };
    let fields = [
        parent,
        sibling,
        tables.next_entry(index, None, Tables::directory_entry),
        tables.next_entry(index, None, Tables::file_entry),
    ];
    directories.write(
        directory.entry,
        &fields.map(u32::to_be_bytes).concat(),
        path_hash(if index == 0 { 0 } else { parent }, directory.name),
        directory.name,
    );
    for (position, child) in directory.children.iter().enumerate() {
        match child {
            Child::Directory(child) => populate(tables, directories, files, *child),
            Child::File(file) => {
                let file = &tables.files[*file];
                let mut fields = Vec::with_capacity(24);
                fields.extend(directory.entry.to_be_bytes());
                fields.extend(
                    tables
                        .next_entry(index, Some(position), Tables::file_entry)
                        .to_be_bytes(),
                );
                fields.extend((file.offset as u64).to_be_bytes());
                fields.extend((file.data.len() as u64).to_be_bytes());
                files.write(
                    file.entry,
                    &fields,
                    path_hash(directory.entry, file.name),
                    file.name,
                );
            }
        }
    }
}

/// Writes the root's children to a WUHB archive.
pub(crate) fn write(nodes: &[Node]) -> Vec<u8> {
    let mut tables = Tables {
        directories: vec![Directory {
            name: "",
            parent: 0,
            children: Vec::new(),
            entry: 0,
        }],
        files: Vec::new(),
    };
    tables.add(0, nodes);
    let mut next = DIRECTORY_ENTRY_SIZE;
    tables.number_directories(0, &mut next);
    let (mut next, mut data_size) = (0, 0);
    tables.number_files(0, &mut next, &mut data_size);

    // wuhbtool counts the root directory twice, which leaves an empty entry at the end of the
    // directory table and one more directory for the hash table size
    let directory_table_size = DIRECTORY_ENTRY_SIZE
        + tables
            .directories
            .iter()
            .map(|directory| entry_size(DIRECTORY_ENTRY_SIZE, directory.name))
            .sum::<u32>();
    let file_table_size: u32 = tables
        .files
        .iter()
        .map(|file| entry_size(FILE_ENTRY_SIZE, file.name))
        .sum();
    let mut directories = HashedTable::new(tables.directories.len() + 1, directory_table_size);
    let mut files = HashedTable::new(tables.files.len(), file_table_size);
    populate(&tables, &mut directories, &mut files, 0);

    let tables_offset = (FILE_PARTITION_OFFSET + data_size).next_multiple_of(4) as u64;
    let sizes = [
        directories.hashes.len() as u64 * 4,
        u64::from(directory_table_size),
        files.hashes.len() as u64 * 4,
        u64::from(file_table_size),
    ];
    let mut archive = b"WUHB".to_vec();
    archive.extend(HEADER_SIZE.to_be_bytes());
    let mut offset = tables_offset;
    for size in sizes {
        archive.extend(offset.to_be_bytes());
        archive.extend(size.to_be_bytes());
        offset += size;
    }
    archive.extend((FILE_PARTITION_OFFSET as u64).to_be_bytes());
    archive.resize(tables_offset as usize, 0);
    for file in &tables.files {
        let start = FILE_PARTITION_OFFSET + file.offset;
        archive[start..start + file.data.len()].copy_from_slice(file.data);
    }
    archive.extend(directories.hashes_bytes());
    archive.extend(&directories.table);
    archive.extend(files.hashes_bytes());
    archive.extend(&files.table);
    archive
}

// MARK: Tests
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_tables_have_no_small_prime_factors() {
        assert_eq!(
            [0, 2, 3, 4, 18, 19, 20, 100].map(hash_table_count),
            [3, 3, 3, 5, 19, 19, 23, 101]
        );
    }
}
