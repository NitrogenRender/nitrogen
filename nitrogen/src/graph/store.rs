use std::any::{Any, TypeId};
use std::collections::HashMap;

pub struct Store {
    map: HashMap<TypeId, Box<dyn Any + Send>>,
}

impl Store {
    pub fn new() -> Self {
        Store {
            map: HashMap::new(),
        }
    }

    pub fn insert<T: Any + Send>(&mut self, data: T) -> Option<T> {
        let id = TypeId::of::<T>();
        let data = Box::new(data);

        self.map
            .insert(id, data)
            .map(|data| *data.downcast().unwrap())
    }

    pub fn get<T: Any + Send>(&self) -> Option<&T> {
        let id = TypeId::of::<T>();

        self.map.get(&id).and_then(|data| data.downcast_ref())
    }

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
    fn re_insert() {
        let mut store = Store::new();

        store.insert(42usize);

        assert_eq!(store.get::<usize>(), Some(&42));

        let old = store.insert(1337usize);

        assert_eq!(old, Some(42));

        assert_eq!(store.get::<usize>(), Some(&1337));
    }

}
