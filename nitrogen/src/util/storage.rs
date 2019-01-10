/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

use slab::Slab;
use std::fmt::{Debug, Formatter};
use std::hash::{Hash, Hasher};
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
impl<T> Hash for Handle<T> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.0.hash(state);
        self.1.hash(state);
    }
}
impl<T> PartialEq for Handle<T> {
    fn eq(&self, other: &Handle<T>) -> bool {
        self.id() == other.id() && self.generation() == other.generation()
    }
}
impl<T> Eq for Handle<T> {}

impl<T> Debug for Handle<T> {
    fn fmt(&self, fmt: &mut Formatter) -> Result<(), std::fmt::Error> {
        fmt.write_fmt(format_args!("Handle({}, {})", self.0, self.1))
    }
}

impl<T> Handle<T> {
    pub unsafe fn new(id: Id, gen: Generation) -> Self {
        Handle(id, gen, PhantomData)
    }

    pub fn id(&self) -> usize {
        self.0
    }

    pub fn generation(&self) -> u64 {
        self.1
    }
}

#[derive(Debug)]
pub(crate) struct Storage<T> {
    pub generations: Vec<Generation>,
    pub entries: Slab<T>,
}

impl<T> Index<Handle<T>> for Storage<T> {
    type Output = T;

    fn index(&self, index: Handle<T>) -> &Self::Output {
        if !self.is_alive(index) {
            panic!("Invalid index on storage: entry is not alive");
        } else {
            &self.entries[index.id()]
        }
    }
}

impl<T> IndexMut<Handle<T>> for Storage<T> {
    fn index_mut(&mut self, index: Handle<T>) -> &mut <Self as Index<Handle<T>>>::Output {
        if !self.is_alive(index) {
            panic!("Invalid index on storage: entry is not alive");
        } else {
            &mut self.entries[index.id()]
        }
    }
}

impl<T> Storage<T> {
    pub(crate) fn new() -> Self {
        Self {
            generations: vec![],
            entries: Slab::new(),
        }
    }

    pub(crate) fn insert(&mut self, data: T) -> Handle<T> {
        let (entry, handle) = {
            let entry = self.entries.vacant_entry();
            let key = entry.key();

            let needs_to_grow = self.generations.len() <= key;

            if needs_to_grow {
                self.generations.push(0);
            } else {
                self.generations[key] += 1;
            }

            let generation = self.generations[key];

            (entry, unsafe { Handle::new(key, generation) })
        };

        entry.insert(data);

        handle
    }

    pub(crate) fn is_alive(&self, handle: Handle<T>) -> bool {
        let storage_size_enough = self.generations.len() > handle.id();

        if storage_size_enough {
            let is_generation_same = self.generations[handle.id()] == handle.generation();
            is_generation_same
        } else {
            false
        }
    }

    pub(crate) fn remove(&mut self, handle: Handle<T>) -> Option<T> {
        if self.is_alive(handle) {
            let data = self.entries.remove(handle.id());
            Some(data)
        } else {
            None
        }
    }

    pub(crate) fn get(&self, handle: Handle<T>) -> Option<&T> {
        if self.is_alive(handle) {
            self.entries.get(handle.id())
        } else {
            None
        }
    }

    pub(crate) fn get_mut(&mut self, handle: Handle<T>) -> Option<&mut T> {
        if self.is_alive(handle) {
            self.entries.get_mut(handle.id())
        } else {
            None
        }
    }
}

use std::iter::IntoIterator;

pub(crate) struct StorageIter<T> {
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
        // In order to iterate we need to go over all possible indices.
        // Because there can be holes in the entry list, we have to
        // search until we find the next element OR we reached the end.

        let mut idx = self.index;

        let len = self.storage.entries.capacity();

        while idx < len {
            if self.storage.entries.contains(idx) {
                // start searching at the next one the next iteration
                self.index = idx + 1;

                let data = self.storage.entries.remove(idx);
                return Some((idx, data));
            }

            idx += 1;
        }

        None
    }
}
