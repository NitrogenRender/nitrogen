use slab::Slab;
use std::marker::PhantomData;
use std::ops::{Index, IndexMut};

pub type Generation = u64;
pub type Id = usize;

pub struct Handle<T>(pub Id, pub Generation, PhantomData<T>);

// huh. Derive doesn't work here because Rust can't prove that `T` is Copy.
// It does work if we implement it manually
impl<T> Copy for Handle<T> {}
impl<T> Clone for Handle<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Handle<T> {
    pub fn new(id: Id, gen: Generation) -> Self {
        Handle(id, gen, PhantomData)
    }

    pub fn id(&self) -> usize {
        self.0
    }

    pub fn generation(&self) -> u64 {
        self.1
    }
}

pub enum InsertOp {
    Grow,
    Inplace,
}

pub struct Storage<T> {
    pub generations: Vec<Generation>,
    pub entries: Slab<T>,
}

impl<T> Index<Handle<T>> for Storage<T> {
    type Output = T;

    fn index(&self, index: Handle<T>) -> &Self::Output {
        if !self.is_alive(index) {
            panic!("Invalid index on storage: entry is no longer alive");
        } else {
            &self.entries[index.id()]
        }
    }
}

impl<T> IndexMut<Handle<T>> for Storage<T> {
    fn index_mut(&mut self, index: Handle<T>) -> &mut <Self as Index<Handle<T>>>::Output {
        if !self.is_alive(index) {
            panic!("Invalid index on storage: entry is no longer alive");
        } else {
            &mut self.entries[index.id()]
        }
    }
}

impl<T> Storage<T> {
    pub fn new() -> Self {
        Self {
            generations: vec![],
            entries: Slab::new(),
        }
    }

    pub fn insert(&mut self, data: T) -> (Handle<T>, InsertOp) {
        let (entry, handle, insert_op) = {
            let entry = self.entries.vacant_entry();
            let key = entry.key();

            let needs_to_grow = self.generations.len() <= key;

            let insert_op = if needs_to_grow {
                InsertOp::Grow
            } else {
                InsertOp::Inplace
            };

            if needs_to_grow {
                self.generations.push(0);
            } else {
                self.generations[key] += 1;
            }

            let generation = self.generations[key];

            (entry, Handle::new(key, generation), insert_op)
        };

        entry.insert(data);

        (handle, insert_op)
    }

    pub fn is_alive(&self, handle: Handle<T>) -> bool {
        let storage_size_enough = self.generations.len() > handle.id();

        if storage_size_enough {
            let is_generation_same = self.generations[handle.id()] == handle.generation();
            is_generation_same
        } else {
            false
        }
    }

    pub fn remove(&mut self, handle: Handle<T>) -> Option<T> {
        if self.is_alive(handle) {
            let data = self.entries.remove(handle.id());
            Some(data)
        } else {
            None
        }
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }
}

use std::iter::IntoIterator;

pub struct StorageIter<T> {
    storage: Storage<T>,
    index: usize,
}

impl<T> IntoIterator for Storage<T> {
    type Item = (usize, T);
    type IntoIter = StorageIter<T>;

    fn into_iter(self) -> <Self as IntoIterator>::IntoIter {
        StorageIter {
            storage: self,
            index: 0,
        }
    }
}

impl<T> Iterator for StorageIter<T> {
    type Item = (usize, T);

    fn next(&mut self) -> Option<(usize, T)> {
        let idx = self.index;

        if idx >= self.storage.entries.len() {
            return None;
        }

        if !self.storage.entries.contains(idx) {
            return None;
        }

        let data = self.storage.entries.remove(idx);

        self.index += 1;

        Some((idx, data))
    }
}
