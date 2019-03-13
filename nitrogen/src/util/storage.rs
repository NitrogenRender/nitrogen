/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

//! A container used for managing data using opaque handles.

use slab::Slab;
use std::fmt::{Debug, Formatter};
use std::hash::{Hash, Hasher};
use std::marker::PhantomData;
use std::ops::{Index, IndexMut};

/// The "generation" of a handle.
///
/// Since entries in a `Storage` can be re-used after an element has been freed, there has to be a
/// mechanism to avoid accidentally referring to old resources while manipulating new ones.
///
/// The generation of a "cell" in the storage will be incremented each time an element is removed
/// from it. This way, the generation refers to a "point in time where an element is alive".
pub type Generation = u64;

/// The "index" of a handle.
///
/// Used to refer to an "element cell" in a `Storage`.
pub type Id = usize;

/// A strongly-typed opaque handle to a value in a `Storage`.
#[repr(C)]
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
    /// Create a new handle by providing an index and a generation.
    ///
    /// Using this method should generally be avoided.
    ///
    /// It is useful for initializing structs which contain handles *without inserting elements into
    /// a store before*. In that case `Option<Handle<T>>` could be used, but since a handle is only
    /// valid for a certain generation, there are no safety violations.
    pub fn new(id: Id, gen: Generation) -> Self {
        Handle(id, gen, PhantomData)
    }

    /// "index" part of the handle.
    pub fn id(&self) -> Id {
        self.0
    }

    /// "generation" part of the handle.
    pub fn generation(&self) -> Generation {
        self.1
    }
}

/// A container used to refer to elements using opaque handles.
#[derive(Debug)]
pub struct Storage<T> {
    generations: Vec<Generation>,
    entries: Slab<T>,
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

impl<T> Default for Storage<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> Storage<T> {
    /// Create a new empty `Storage`, containing no elements.
    pub fn new() -> Self {
        Self {
            generations: vec![],
            entries: Slab::new(),
        }
    }

    /// Insert an element into the store, returning a `Handle` to the element.
    pub fn insert(&mut self, data: T) -> Handle<T> {
        let (entry, handle) = {
            let entry = self.entries.vacant_entry();
            let key = entry.key();

            let needs_to_grow = self.generations.len() <= key;

            if needs_to_grow {
                self.generations.push(0);
            }

            let generation = self.generations[key];

            (entry, Handle::new(key, generation))
        };

        entry.insert(data);

        handle
    }

    /// Checks if an element pointed to by `handle` is alive.
    pub fn is_alive(&self, handle: Handle<T>) -> bool {
        let storage_size_enough = self.generations.len() > handle.id();

        if storage_size_enough {
            // is the generation the same?
            self.generations[handle.id()] == handle.generation()
        } else {
            false
        }
    }

    /// Remove an element from the `Storage`, return the removed value if it exists.
    pub fn remove(&mut self, handle: Handle<T>) -> Option<T> {
        if self.is_alive(handle) {
            let data = self.entries.remove(handle.id());
            self.generations[handle.id()] += 1;
            Some(data)
        } else {
            None
        }
    }

    /// Retrieve a shared reference to the element pointed at by `handle`, if it is alive.
    pub fn get(&self, handle: Handle<T>) -> Option<&T> {
        if self.is_alive(handle) {
            self.entries.get(handle.id())
        } else {
            None
        }
    }

    /// Retrieve a mutable/unique reference to the element pointed at by `handle`, if it is alive.
    pub fn get_mut(&mut self, handle: Handle<T>) -> Option<&mut T> {
        if self.is_alive(handle) {
            self.entries.get_mut(handle.id())
        } else {
            None
        }
    }
}

use std::iter::IntoIterator;

/// An iterator over the elements of a [`Storage`].
///
/// [`Storage`]: ./struct.Storage.html
pub struct StorageIntoIter<T> {
    storage: Storage<T>,
    index: Id,
}

impl<T> IntoIterator for Storage<T> {
    type Item = (Id, T);
    type IntoIter = StorageIntoIter<T>;

    fn into_iter(self) -> <Self as IntoIterator>::IntoIter {
        StorageIntoIter {
            storage: self,
            index: 0,
        }
    }
}

impl<T> Iterator for StorageIntoIter<T> {
    type Item = (Id, T);

    fn next(&mut self) -> Option<(Id, T)> {
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

/// Iterator over an immutably borrowed storage.
pub struct StorageIter<'a, T> {
    storage: &'a Storage<T>,
    index: Id,
}

impl<'a, T> IntoIterator for &'a Storage<T> {
    type Item = (Id, &'a T);
    type IntoIter = StorageIter<'a, T>;

    fn into_iter(self) -> Self::IntoIter {
        StorageIter {
            storage: self,
            index: 0,
        }
    }
}

impl<'a, T> Iterator for StorageIter<'a, T> {
    type Item = (Id, &'a T);

    fn next(&mut self) -> Option<(Id, &'a T)> {
        // In order to iterate we need to go over all possible indices.
        // Because there can be holes in the entry list, we have to
        // search until we find the next element OR we reached the end.

        let mut idx = self.index;

        let len = self.storage.entries.capacity();

        while idx < len {
            if self.storage.entries.contains(idx) {
                // start searching at the next one the next iteration
                self.index = idx + 1;

                let data = &self.storage.entries[idx];
                return Some((idx, data));
            }

            idx += 1;
        }

        None
    }
}
