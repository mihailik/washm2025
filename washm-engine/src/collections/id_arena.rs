use std::marker::PhantomData;
use std::ops::{Index, IndexMut};

/// A generational ID that points to an item in an `IdArena`.
pub struct Id<T> {
    index: u32,
    generation: u32,
    _marker: PhantomData<fn() -> T>,
}

impl<T> Clone for Id<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for Id<T> {}

impl<T> PartialEq for Id<T> {
    fn eq(&self, other: &Self) -> bool {
        self.index == other.index && self.generation == other.generation
    }
}

impl<T> Eq for Id<T> {}

impl<T> std::hash::Hash for Id<T> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.index.hash(state);
        self.generation.hash(state);
    }
}

impl<T> std::fmt::Debug for Id<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Id")
            .field("index", &self.index)
            .field("generation", &self.generation)
            .finish()
    }
}

impl<T> Id<T> {
    /// Create a new ID from indices. Internal use only usually.
    pub fn from_raw_parts(index: u32, generation: u32) -> Self {
        Self {
            index,
            generation,
            _marker: PhantomData,
        }
    }

    pub fn index(self) -> u32 {
        self.index
    }

    pub fn generation(self) -> u32 {
        self.generation
    }

    /// Cast this ID to another type marker.
    pub fn cast<U>(self) -> Id<U> {
        Id::from_raw_parts(self.index, self.generation)
    }
}

/// A lightweight generational arena.
#[derive(Debug, Clone)]
pub struct IdArena<T> {
    items: Vec<Entry<T>>,
    free_head: Option<u32>,
    len: usize,
}

#[derive(Debug, Clone)]
enum Entry<T> {
    Free { next: Option<u32>, generation: u32 },
    Occupied { value: T, generation: u32 },
}

impl<T> IdArena<T> {
    /// Create a new empty arena.
    pub fn new() -> Self {
        Self {
            items: Vec::new(),
            free_head: None,
            len: 0,
        }
    }

    /// Allocate a new item in the arena and return its ID.
    pub fn alloc(&mut self, value: T) -> Id<T> {
        self.len += 1;
        if let Some(index) = self.free_head {
            let entry = &mut self.items[index as usize];
            if let Entry::Free { next, generation } = entry {
                let gen = *generation;
                self.free_head = *next;
                *entry = Entry::Occupied {
                    value,
                    generation: gen,
                };
                Id::from_raw_parts(index, gen)
            } else {
                panic!("Arena free list corrupted");
            }
        } else {
            let index = self.items.len() as u32;
            let generation = 0;
            self.items.push(Entry::Occupied {
                value,
                generation,
            });
            Id::from_raw_parts(index, generation)
        }
    }

    /// Deallocate an item by its ID.
    /// Returns the item if the ID was valid.
    pub fn dealloc(&mut self, id: Id<T>) -> Option<T> {
        let index = id.index as usize;
        if index >= self.items.len() {
            return None;
        }

        match &self.items[index] {
            Entry::Occupied { generation, .. } if *generation == id.generation => {
                let gen = *generation;
                let val = std::mem::replace(
                    &mut self.items[index],
                    Entry::Free {
                        next: self.free_head,
                        generation: gen + 1,
                    },
                );
                self.free_head = Some(id.index);
                self.len -= 1;
                if let Entry::Occupied { value, .. } = val {
                    Some(value)
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    /// Get a reference to an item by its ID.
    pub fn get(&self, id: Id<T>) -> Option<&T> {
        match self.items.get(id.index as usize)? {
            Entry::Occupied { value, generation } if *generation == id.generation => Some(value),
            _ => None,
        }
    }

    /// Get a mutable reference to an item by its ID.
    pub fn get_mut(&mut self, id: Id<T>) -> Option<&mut T> {
        match self.items.get_mut(id.index as usize)? {
            Entry::Occupied { value, generation } if *generation == id.generation => Some(value),
            _ => None,
        }
    }

    /// Get the ID for an item at a specific raw index.
    /// This is mainly for legacy bridge purposes and should be used with caution.
    pub fn id_from_index(&self, index: u32) -> Option<Id<T>> {
        match self.items.get(index as usize)? {
            Entry::Occupied { generation, .. } => Some(Id::from_raw_parts(index, *generation)),
            _ => None,
        }
    }

    /// Returns the number of occupied items in the arena.
    pub fn len(&self) -> usize {
        self.len
    }

    /// Returns true if the arena is empty.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn ids(&self) -> impl Iterator<Item = Id<T>> + '_ {
        self.items.iter().enumerate().filter_map(|(i, entry)| {
            if let Entry::Occupied { generation, .. } = entry {
                Some(Id::from_raw_parts(i as u32, *generation))
            } else {
                None
            }
        })
    }

    /// Iterate over all occupied items and their IDs.
    pub fn iter(&self) -> impl Iterator<Item = (Id<T>, &T)> {
        self.items
            .iter()
            .enumerate()
            .filter_map(|(i, entry)| match entry {
                Entry::Occupied { value, generation } => Some((
                    Id::from_raw_parts(i as u32, *generation),
                    value,
                )),
                _ => None,
            })
    }

    /// Iterate over all occupied items and their IDs mutably.
    pub fn iter_mut(&mut self) -> impl Iterator<Item = (Id<T>, &mut T)> {
        self.items
            .iter_mut()
            .enumerate()
            .filter_map(|(i, entry)| match entry {
                Entry::Occupied { value, generation } => Some((
                    Id::from_raw_parts(i as u32, *generation),
                    value,
                )),
                _ => None,
            })
    }
}

impl<T> Index<Id<T>> for IdArena<T> {
    type Output = T;
    fn index(&self, id: Id<T>) -> &Self::Output {
        self.get(id).expect("Invalid ID used to index arena")
    }
}

impl<T> IndexMut<Id<T>> for IdArena<T> {
    fn index_mut(&mut self, id: Id<T>) -> &mut Self::Output {
        self.get_mut(id).expect("Invalid ID used to index arena")
    }
}

impl<T> Default for IdArena<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<'a, T> IntoIterator for &'a IdArena<T> {
    type Item = (Id<T>, &'a T);
    type IntoIter = Box<dyn Iterator<Item = Self::Item> + 'a>;

    fn into_iter(self) -> Self::IntoIter {
        Box::new(self.iter())
    }
}

impl<'a, T> IntoIterator for &'a mut IdArena<T> {
    type Item = (Id<T>, &'a mut T);
    type IntoIter = Box<dyn Iterator<Item = Self::Item> + 'a>;

    fn into_iter(self) -> Self::IntoIter {
        Box::new(self.iter_mut())
    }
}
