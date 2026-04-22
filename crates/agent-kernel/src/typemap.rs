use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Default, Clone)]
pub struct TypeMap {
    map: HashMap<TypeId, Arc<dyn Any + Send + Sync>>,
}

impl TypeMap {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert<T: Send + Sync + 'static>(&mut self, val: T) {
        self.map.insert(TypeId::of::<T>(), Arc::new(val));
    }

    pub fn get<T: 'static>(&self) -> Option<&T> {
        self.map
            .get(&TypeId::of::<T>())
            .and_then(|arc| arc.downcast_ref::<T>())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_typemap_insert_and_retrieve() {
        let mut map = TypeMap::new();
        map.insert("hello".to_string());
        assert_eq!(map.get::<String>().unwrap(), "hello");
    }
}
