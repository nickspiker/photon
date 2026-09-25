//! A map by LINEAR SEARCH over a `Vec` of pairs (AGENT.md: no HashMap without consent and a proof it beats a linear search).
//! The maps that use it are keyed by friends, devices, lanes, pipeline stages or requests in flight — tens of entries, where one scan over contiguous pairs costs no more than hashing the key, and iteration follows insertion order instead of a per-process random one.
//! The method names match `HashMap`'s, so a converted call site reads the same.

use std::borrow::Borrow;

#[derive(Clone, Debug, PartialEq)]
pub struct LinearMap<K, V> {
    entries: Vec<(K, V)>,
}

impl<K, V> Default for LinearMap<K, V> {
    fn default() -> Self {
        Self { entries: Vec::new() }
    }
}

impl<K: PartialEq, V> LinearMap<K, V> {
    pub const fn new() -> Self {
        Self { entries: Vec::new() }
    }

    fn position<Q: PartialEq + ?Sized>(&self, k: &Q) -> Option<usize>
    where
        K: Borrow<Q>,
    {
        self.entries.iter().position(|(key, _)| key.borrow() == k)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }

    pub fn get<Q: PartialEq + ?Sized>(&self, k: &Q) -> Option<&V>
    where
        K: Borrow<Q>,
    {
        self.entries.iter().find(|(key, _)| key.borrow() == k).map(|(_, v)| v)
    }

    pub fn get_mut<Q: PartialEq + ?Sized>(&mut self, k: &Q) -> Option<&mut V>
    where
        K: Borrow<Q>,
    {
        self.entries.iter_mut().find(|(key, _)| (*key).borrow() == k).map(|(_, v)| v)
    }

    pub fn contains_key<Q: PartialEq + ?Sized>(&self, k: &Q) -> bool
    where
        K: Borrow<Q>,
    {
        self.position(k).is_some()
    }

    /// Insert or replace; the replaced value comes back, as `HashMap::insert`.
    pub fn insert(&mut self, k: K, v: V) -> Option<V> {
        match self.position(&k) {
            Some(i) => Some(std::mem::replace(&mut self.entries[i].1, v)),
            None => {
                self.entries.push((k, v));
                None
            }
        }
    }

    /// Remove the entry, keeping the others in insertion order.
    pub fn remove<Q: PartialEq + ?Sized>(&mut self, k: &Q) -> Option<V>
    where
        K: Borrow<Q>,
    {
        let i = self.position(k)?;
        Some(self.entries.remove(i).1)
    }

    pub fn entry(&mut self, k: K) -> Entry<'_, K, V> {
        let at = self.position(&k);
        Entry { map: self, key: k, at }
    }

    pub fn retain(&mut self, mut keep: impl FnMut(&K, &mut V) -> bool) {
        self.entries.retain_mut(|(k, v)| keep(k, v));
    }

    pub fn iter(&self) -> impl Iterator<Item = (&K, &V)> {
        self.entries.iter().map(|(k, v)| (k, v))
    }

    pub fn iter_mut(&mut self) -> impl Iterator<Item = (&K, &mut V)> {
        self.entries.iter_mut().map(|(k, v)| (&*k, v))
    }

    pub fn keys(&self) -> impl Iterator<Item = &K> {
        self.entries.iter().map(|(k, _)| k)
    }

    pub fn values(&self) -> impl Iterator<Item = &V> {
        self.entries.iter().map(|(_, v)| v)
    }

    pub fn values_mut(&mut self) -> impl Iterator<Item = &mut V> {
        self.entries.iter_mut().map(|(_, v)| v)
    }

    pub fn drain(&mut self) -> impl Iterator<Item = (K, V)> + '_ {
        self.entries.drain(..)
    }
}

impl<K: PartialEq, V> FromIterator<(K, V)> for LinearMap<K, V> {
    fn from_iter<I: IntoIterator<Item = (K, V)>>(iter: I) -> Self {
        let mut m = Self::new();
        for (k, v) in iter {
            m.insert(k, v);
        }
        m
    }
}

impl<K, V> IntoIterator for LinearMap<K, V> {
    type Item = (K, V);
    type IntoIter = std::vec::IntoIter<(K, V)>;
    fn into_iter(self) -> Self::IntoIter {
        self.entries.into_iter()
    }
}

/// `HashMap::entry`'s shape: the key, and where it already sits (if it does).
pub struct Entry<'a, K, V> {
    map: &'a mut LinearMap<K, V>,
    key: K,
    at: Option<usize>,
}

impl<'a, K: PartialEq, V> Entry<'a, K, V> {
    pub fn or_insert_with(self, make: impl FnOnce() -> V) -> &'a mut V {
        let i = match self.at {
            Some(i) => i,
            None => {
                self.map.entries.push((self.key, make()));
                self.map.entries.len() - 1 // PROOF: the push above left at least one entry
            }
        };
        &mut self.map.entries[i].1
    }

    pub fn or_insert(self, v: V) -> &'a mut V {
        self.or_insert_with(|| v)
    }

    pub fn or_default(self) -> &'a mut V
    where
        V: Default,
    {
        self.or_insert_with(V::default)
    }

    pub fn and_modify(self, f: impl FnOnce(&mut V)) -> Self {
        if let Some(i) = self.at {
            f(&mut self.map.entries[i].1);
        }
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The HashMap surface behaves the same: insert replaces and returns the old value, entry finds or creates, remove keeps the rest in insertion order.
    #[test]
    fn behaves_like_the_map_it_replaces() {
        let mut m: LinearMap<u8, u32> = LinearMap::new();
        assert_eq!(m.insert(3, 30), None);
        assert_eq!(m.insert(1, 10), None);
        assert_eq!(m.insert(3, 31), Some(30));
        *m.entry(2).or_insert(0) += 5;
        *m.entry(2).or_insert(0) += 5;
        m.entry(1).and_modify(|v| *v += 1).or_insert(0);
        assert_eq!(m.iter().map(|(k, v)| (*k, *v)).collect::<Vec<_>>(), vec![(3, 31), (1, 11), (2, 10)]);
        assert_eq!(m.remove(&1), Some(11));
        assert_eq!(m.keys().copied().collect::<Vec<_>>(), vec![3, 2]);
        m.retain(|_, v| *v > 20);
        assert_eq!(m.len(), 1);
        assert_eq!(m.get(&3), Some(&31));
    }
}
