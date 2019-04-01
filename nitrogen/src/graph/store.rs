/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

//! A general purpose associated container, mapping from **type** to a single values.

use std::any::{Any, TypeId};
use std::collections::HashMap;

use std::marker::PhantomData;

/// A general purpose per-type-storage intended for passing data into passes.
///
/// For the most prominent usage, see [`SubmitGroup::graph_execute`].
///
/// [`SubmitGroup::graph_execute`]: ../../util/submit_group/struct.SubmitGroup.html#method.graph_execute
#[derive(Default)]
pub struct Store {
    map: HashMap<TypeId, Box<dyn Any + Send>>,
}

impl Store {
    /// Create a new, empty Store
    ///
    /// # Examples
    ///
    /// ```
    /// # use nitrogen::graph::Store;
    /// let store = Store::new();
    /// ```
    pub fn new() -> Self {
        Default::default()
    }

    /// Insert a value into the store
    ///
    /// # Examples
    ///
    /// ```
    /// # use nitrogen::graph::Store;
    /// let mut store = Store::new();
    /// store.insert::<u8>(12);
    /// ```
    pub fn insert<T: Any + Send>(&mut self, data: T) -> Option<T> {
        let id = TypeId::of::<T>();
        let data = Box::new(data);

        self.map
            .insert(id, data)
            .map(|data| *data.downcast().unwrap())
    }

    /// Retrieve a reference to a value from the store
    ///
    /// # Examples
    ///
    /// ```
    /// # use nitrogen::graph::Store;
    /// let mut store = Store::new();
    /// store.insert::<usize>(1180);
    ///
    /// match store.get::<usize>() {
    ///     Some(val) => println!("Got {}", val),
    ///     None => println!("No value in store..."),
    /// }
    /// ```
    pub fn get<T: Any + Send>(&self) -> Option<&T> {
        let id = TypeId::of::<T>();

        self.map.get(&id).and_then(|data| data.downcast_ref())
    }

    /// Retrieve a mutable reference to a value from the store
    ///
    /// # Examples
    ///
    /// ```
    /// # use nitrogen::graph::Store;
    /// let mut store = Store::new();
    /// store.insert::<u8>(0);
    ///
    /// *store.get_mut::<u8>().unwrap() += 1;
    ///
    /// assert_eq!(store.get::<u8>(), Some(&1));
    /// ```
    pub fn get_mut<T: Any + Send>(&mut self) -> Option<&mut T> {
        let id = TypeId::of::<T>();

        self.map.get_mut(&id).and_then(|data| data.downcast_mut())
    }

    /// Remove an entry from the store
    ///
    /// # Examples
    ///
    /// ```
    /// # use nitrogen::graph::Store;
    /// let mut store = Store::new();
    /// store.insert::<bool>(true);
    ///
    /// assert_eq!(store.remove::<bool>(), Some(true));
    ///
    /// assert_eq!(store.get::<bool>(), None);
    /// ```
    pub fn remove<T: Any + Send>(&mut self) -> Option<T> {
        let id = TypeId::of::<T>();

        let old_data = self.map.remove(&id);

        old_data.and_then(|data| {
            let data = data.downcast::<T>();

            match data {
                Ok(ret) => Some(*ret),
                Err(_) => None,
            }
        })
    }

    /// Return an entry object for in-place insertion of elements
    ///
    /// # Examples
    ///
    /// ```
    /// # use nitrogen::graph::Store;
    /// let mut store = Store::new();
    /// store.entry::<i16>().or_insert(15);
    ///
    /// assert_eq!(store.get::<i16>(), Some(&15));
    ///
    /// // There already is an element, so no insertion will happen
    /// store.entry::<i16>().or_insert_with(|| 30);
    ///
    /// assert_eq!(store.get::<i16>(), Some(&15));
    /// ```
    pub fn entry<T: Any + Send>(&mut self) -> Entry<'_, T> {
        let id = TypeId::of::<T>();

        Entry {
            entry: self.map.entry(id),
            _marker: PhantomData,
        }
    }

    /// Remove all entries from the store
    pub fn clear(&mut self) {
        self.map.clear();
    }

    /// Return the number of entries in the store
    pub fn len(&self) -> usize {
        self.map.len()
    }

    /// Indicates if the store is empty
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Representation of an entry of a [`Store`] that can be used to modify elements inplace
/// or insert them.
///
/// [`Store`]: ./struct.Store.html
pub struct Entry<'a, T: Any + Send> {
    entry: ::std::collections::hash_map::Entry<'a, TypeId, Box<dyn Any + Send>>,
    _marker: PhantomData<T>,
}

impl<'a, T: Any + Send> Entry<'a, T> {
    /// Insert an element into the entry if none exists yet. The element value is computed
    /// **lazily** by calling a function passed as an argument.
    ///
    /// # Examples
    ///
    /// ```
    /// # use nitrogen::graph::Store;
    /// let mut store = Store::new();
    ///
    /// // The callback is only run and the result inserted when no element exists yet
    /// store.entry::<u32>()
    ///     .or_insert_with(|| {
    ///         // potentially complex computations
    ///         42
    ///     });
    ///
    /// assert_eq!(store.get::<u32>(), Some(&42));
    /// ```
    pub fn or_insert_with<F: FnOnce() -> T>(self, f: F) -> &'a mut T {
        let entry = self.entry.or_insert_with(|| {
            let val = f();
            Box::new(val)
        });
        entry.downcast_mut().unwrap()
    }

    /// Insert a value into the entry if none exists yet.
    ///
    /// # Examples
    ///
    /// ```
    /// # use nitrogen::graph::Store;
    /// let mut store = Store::new();
    /// store.entry::<bool>().or_insert(false);
    /// assert_eq!(store.get::<bool>(), Some(&false));
    ///
    /// store.entry::<bool>().or_insert(true);
    /// assert_eq!(store.get::<bool>(), Some(&false));
    /// ```
    pub fn or_insert(self, data: T) -> &'a mut T {
        let entry = self.entry.or_insert_with(|| Box::new(data));

        entry.downcast_mut().unwrap()
    }

    /// Modify the element, if it exists, inplace.
    ///
    /// # Examples
    ///
    /// ```
    /// # use nitrogen::graph::Store;
    /// let mut store = Store::new();
    /// store.entry::<u8>().or_insert(12);
    ///
    /// store.entry::<u8>().and_modify(|val| *val += 1);
    ///
    /// assert_eq!(store.get::<u8>(), Some(&13));
    /// ```
    pub fn and_modify<F: FnOnce(&mut T)>(self, f: F) -> Self {
        let entry = self
            .entry
            .and_modify(|data| f(data.downcast_mut().unwrap()));

        Entry {
            entry,
            _marker: PhantomData,
        }
    }
}

#[cfg(test)]
mod tests {

    use super::*;

    #[test]
    fn simple_insert() {
        #[derive(Debug, Eq, PartialEq)]
        struct A(bool);

        let mut store = Store::new();

        store.insert(A(true));

        assert_eq!(store.get::<A>(), Some(&A(true)));
    }

    #[test]
    fn simple_not_existing() {
        #[derive(Debug, Eq, PartialEq)]
        struct A(bool);

        let store = Store::new();

        assert_eq!(store.get::<A>(), None);
    }

    #[test]
    fn two_types() {
        let mut store = Store::new();

        store.insert(42_u32);
        store.insert(1337_usize);

        assert_eq!(store.get::<u32>(), Some(&42));
        assert_eq!(store.get::<usize>(), Some(&1337));
    }

    #[test]
    fn remove() {
        let mut store = Store::new();

        store.insert::<bool>(true);

        assert_eq!(store.get::<bool>(), Some(&true));

        store.remove::<bool>();

        assert_eq!(store.get::<bool>(), None);
    }

    #[test]
    fn get_mut() {
        let mut store = Store::new();

        store.insert(42usize);

        {
            let elem = store.get_mut::<usize>();

            assert_eq!(elem, Some(&mut 42));
        }

        if let Some(val) = store.get_mut::<usize>() {
            *val = 1337;
        }

        {
            let elem = store.get::<usize>();
            assert_eq!(elem, Some(&1337));
        }

        if let Some(val) = store.get_mut::<u32>() {
            *val = 12;
        };

        assert_eq!(store.get::<u32>(), None);
    }

    #[test]
    fn re_insert() {
        let mut store = Store::new();

        store.insert(42usize);

        assert_eq!(store.get::<usize>(), Some(&42));

        let old = store.insert(1337usize);

        assert_eq!(old, Some(42));

        assert_eq!(store.get::<usize>(), Some(&1337));
    }

    #[test]
    fn entry() {
        {
            let mut store = Store::new();

            let len = store
                .entry::<String>()
                .or_insert_with(|| "Hello".into())
                .len();
            assert_eq!(len, "Hello".len());

            // entry already exists, so no override should happen
            let len = store
                .entry::<String>()
                .or_insert_with(|| "AnotherHello".into())
                .len();
            assert_eq!(len, "Hello".len());
        }

        {
            let mut store = Store::new();

            {
                let elem = store.entry::<usize>().or_insert(42);

                assert_eq!(*elem, 42);
            }

            {
                let elem = store.entry::<usize>().or_insert(1337);
                assert_eq!(*elem, 42);
            }
        }

        // insert, then entry
        {
            let mut store = Store::new();

            store.insert("Hello".to_string());

            let elem = store.entry::<String>().or_insert_with(|| "World!".into());

            assert_eq!(elem, "Hello");
        }

        // entry, remove, entry again
        {
            let mut store = Store::new();

            store.entry::<String>().or_insert("Hello".to_string());

            store.remove::<String>();

            let elem = store.entry::<String>().or_insert_with(|| "World!".into());

            assert_eq!(elem, "World!");
        }

        {
            let mut store = Store::new();

            store.entry::<u8>().and_modify(|x| *x = 12);

            assert_eq!(store.get::<u8>(), None);

            store.insert::<u8>(0);

            store
                .entry::<u8>()
                .and_modify(|x| *x += 1)
                .and_modify(|x| *x *= 2);

            assert_eq!(store.get::<u8>(), Some(&2));
        }
    }

    #[test]
    fn clear() {
        let mut store = Store::new();

        assert_eq!(store.len(), 0);

        store.insert(12i8);

        assert_eq!(store.len(), 1);

        store.insert::<f64>(4.2);

        assert_eq!(store.len(), 2);

        store.clear();

        assert_eq!(store.len(), 0);

        assert_eq!(store.get::<i8>(), None);
        assert_eq!(store.get::<f64>(), None);
    }

}
