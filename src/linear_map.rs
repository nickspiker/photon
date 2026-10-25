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

/// A set by LINEAR SEARCH over a `Vec` — `LinearMap`'s twin for membership (AGENT.md: no HashSet without consent and proof); method names match `HashSet`'s.
#[derive(Clone, Debug, PartialEq)]
pub struct LinearSet<K> {
    items: Vec<K>,
}

impl<K> Default for LinearSet<K> {
    fn default() -> Self {
        Self { items: Vec::new() }
    }
}

impl<K: PartialEq> LinearSet<K> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn contains(&self, k: &K) -> bool {
        self.items.contains(k)
    }

    /// True when `k` was not yet present (it is now).
    pub fn insert(&mut self, k: K) -> bool {
        if self.items.contains(&k) {
            return false;
        }
        self.items.push(k);
        true
    }

    /// True when `k` was present (it is gone now).
    pub fn remove(&mut self, k: &K) -> bool {
        match self.items.iter().position(|x| x == k) {
            Some(i) => {
                self.items.swap_remove(i);
                true
            }
            None => false,
        }
    }

    pub fn retain(&mut self, keep: impl FnMut(&K) -> bool) {
        self.items.retain(keep);
    }

    pub fn clear(&mut self) {
        self.items.clear();
    }

    pub fn iter(&self) -> impl Iterator<Item = &K> {
        self.items.iter()
    }
}

/// A map by BINARY SEARCH over a sorted `Vec` of pairs — the simple lookup for keys that grow with history (blob hashes, rows), where a linear scan would not stay cheap (AGENT.md: no HashMap without consent and proof). Method names match `HashMap`'s; iteration runs in key order.
#[derive(Clone, Debug, PartialEq)]
pub struct SortedMap<K, V> {
    pairs: Vec<(K, V)>,
}

impl<K, V> Default for SortedMap<K, V> {
    fn default() -> Self {
        Self { pairs: Vec::new() }
    }
}

impl<K: Ord, V> SortedMap<K, V> {
    pub fn new() -> Self {
        Self::default()
    }

    fn find(&self, k: &K) -> Result<usize, usize> {
        self.pairs.binary_search_by(|(x, _)| x.cmp(k))
    }

    pub fn len(&self) -> usize {
        self.pairs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.pairs.is_empty()
    }

    pub fn clear(&mut self) {
        self.pairs.clear();
    }

    pub fn get(&self, k: &K) -> Option<&V> {
        self.find(k).ok().map(|i| &self.pairs[i].1)
    }

    pub fn get_mut(&mut self, k: &K) -> Option<&mut V> {
        self.find(k).ok().map(move |i| &mut self.pairs[i].1)
    }

    pub fn contains_key(&self, k: &K) -> bool {
        self.find(k).is_ok()
    }

    pub fn insert(&mut self, k: K, v: V) -> Option<V> {
        match self.find(&k) {
            Ok(i) => Some(std::mem::replace(&mut self.pairs[i].1, v)),
            Err(i) => {
                self.pairs.insert(i, (k, v));
                None
            }
        }
    }

    pub fn remove(&mut self, k: &K) -> Option<V> {
        self.find(k).ok().map(|i| self.pairs.remove(i).1)
    }

    /// The value for `k`, inserting `make()` first when absent.
    pub fn entry_or_insert_with(&mut self, k: K, make: impl FnOnce() -> V) -> &mut V {
        let i = match self.find(&k) {
            Ok(i) => i,
            Err(i) => {
                self.pairs.insert(i, (k, make()));
                i
            }
        };
        &mut self.pairs[i].1
    }

    pub fn retain(&mut self, mut keep: impl FnMut(&K, &mut V) -> bool) {
        self.pairs.retain_mut(|(k, v)| keep(k, v));
    }

    pub fn iter(&self) -> impl Iterator<Item = (&K, &V)> {
        self.pairs.iter().map(|(k, v)| (k, v))
    }

    pub fn keys(&self) -> impl Iterator<Item = &K> {
        self.pairs.iter().map(|(k, _)| k)
    }

    pub fn values(&self) -> impl Iterator<Item = &V> {
        self.pairs.iter().map(|(_, v)| v)
    }

    pub fn values_mut(&mut self) -> impl Iterator<Item = &mut V> {
        self.pairs.iter_mut().map(|(_, v)| v)
    }

    /// `HashMap::entry`'s shape: the slot for `k`, filled on the first or_insert*.
    pub fn entry(&mut self, k: K) -> SortedEntry<'_, K, V> {
        SortedEntry { map: self, key: k }
    }
}

pub struct SortedEntry<'a, K, V> {
    map: &'a mut SortedMap<K, V>,
    key: K,
}

impl<'a, K: Ord, V> SortedEntry<'a, K, V> {
    pub fn or_insert_with(self, make: impl FnOnce() -> V) -> &'a mut V {
        self.map.entry_or_insert_with(self.key, make)
    }

    pub fn or_insert(self, v: V) -> &'a mut V {
        self.map.entry_or_insert_with(self.key, || v)
    }

    pub fn or_default(self) -> &'a mut V
    where
        V: Default,
    {
        self.map.entry_or_insert_with(self.key, V::default)
    }

    pub fn and_modify(self, f: impl FnOnce(&mut V)) -> Self {
        if let Some(v) = self.map.get_mut(&self.key) {
            f(v);
        }
        self
    }
}

/// A set by BINARY SEARCH over a sorted `Vec` — `SortedMap`'s twin for membership; method names match `HashSet`'s.
#[derive(Clone, Debug, PartialEq)]
pub struct SortedSet<K> {
    items: Vec<K>,
}

impl<K> Default for SortedSet<K> {
    fn default() -> Self {
        Self { items: Vec::new() }
    }
}

impl<K: Ord> SortedSet<K> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn contains(&self, k: &K) -> bool {
        self.items.binary_search(k).is_ok()
    }

    /// True when `k` was not yet present (it is now).
    pub fn insert(&mut self, k: K) -> bool {
        match self.items.binary_search(&k) {
            Ok(_) => false,
            Err(i) => {
                self.items.insert(i, k);
                true
            }
        }
    }

    /// True when `k` was present (it is gone now).
    pub fn remove(&mut self, k: &K) -> bool {
        match self.items.binary_search(k) {
            Ok(i) => {
                self.items.remove(i);
                true
            }
            Err(_) => false,
        }
    }

    pub fn retain(&mut self, keep: impl FnMut(&K) -> bool) {
        self.items.retain(keep);
    }

    pub fn clear(&mut self) {
        self.items.clear();
    }

    pub fn iter(&self) -> impl Iterator<Item = &K> {
        self.items.iter()
    }
}

impl<K: Ord> Extend<K> for SortedSet<K> {
    fn extend<I: IntoIterator<Item = K>>(&mut self, it: I) {
        for k in it {
            self.insert(k);
        }
    }
}

impl<K: Ord> FromIterator<K> for SortedSet<K> {
    fn from_iter<I: IntoIterator<Item = K>>(it: I) -> Self {
        let mut items: Vec<K> = it.into_iter().collect();
        items.sort_unstable();
        items.dedup();
        Self { items }
    }
}

#[cfg(test)]
mod sorted_tests {
    use super::*;

    #[test]
    fn sorted_map_and_set_behave_like_the_hashed_ones() {
        let mut m: SortedMap<u32, &str> = SortedMap::new();
        assert_eq!(m.insert(5, "five"), None);
        assert_eq!(m.insert(1, "one"), None);
        assert_eq!(m.insert(5, "FIVE"), Some("five"));
        assert_eq!(m.get(&5), Some(&"FIVE"));
        assert_eq!(m.keys().copied().collect::<Vec<_>>(), vec![1, 5]);
        *m.entry_or_insert_with(3, || "three") = "3";
        assert_eq!(m.remove(&1), Some("one"));
        assert_eq!(m.iter().map(|(k, v)| (*k, *v)).collect::<Vec<_>>(), vec![(3, "3"), (5, "FIVE")]);
        let mut s: SortedSet<u32> = SortedSet::new();
        assert!(s.insert(9) && s.insert(2) && !s.insert(9));
        assert!(s.contains(&2) && !s.contains(&3));
        assert!(s.remove(&9) && !s.remove(&9));
        let mut l: LinearSet<u32> = LinearSet::new();
        assert!(l.insert(4) && !l.insert(4) && l.contains(&4) && l.remove(&4) && l.is_empty());
    }
}

impl<K: Ord, V> SortedMap<K, V> {
    /// `HashMap::with_capacity`'s shape: room reserved for `n` pairs.
    pub fn with_capacity(n: usize) -> Self {
        Self { pairs: Vec::with_capacity(n) }
    }

    pub fn iter_mut(&mut self) -> impl Iterator<Item = (&K, &mut V)> {
        self.pairs.iter_mut().map(|(k, v)| (&*k, v))
    }

    pub fn drain(&mut self) -> impl Iterator<Item = (K, V)> + '_ {
        self.pairs.drain(..)
    }

    pub fn into_keys(self) -> impl Iterator<Item = K> {
        self.pairs.into_iter().map(|(k, _)| k)
    }

    pub fn into_values(self) -> impl Iterator<Item = V> {
        self.pairs.into_iter().map(|(_, v)| v)
    }
}

impl<K: Ord, V> FromIterator<(K, V)> for SortedMap<K, V> {
    /// One sort for the whole batch (inserting one by one in random order would shift the tail every time); for a repeated key the LAST value wins, as `HashMap` would leave it.
    fn from_iter<I: IntoIterator<Item = (K, V)>>(it: I) -> Self {
        let mut pairs: Vec<(K, V)> = it.into_iter().collect();
        // A stable sort keeps repeats in arrival order, so the last of each run is the newest.
        pairs.sort_by(|a, b| a.0.cmp(&b.0));
        let mut out: Vec<(K, V)> = Vec::with_capacity(pairs.len());
        for p in pairs {
            match out.last_mut() {
                Some(last) if last.0 == p.0 => *last = p,
                _ => out.push(p),
            }
        }
        Self { pairs: out }
    }
}

impl<K: Ord, V> Extend<(K, V)> for SortedMap<K, V> {
    fn extend<I: IntoIterator<Item = (K, V)>>(&mut self, it: I) {
        for (k, v) in it {
            self.insert(k, v);
        }
    }
}

impl<K: Ord, V> std::ops::Index<&K> for SortedMap<K, V> {
    type Output = V;
    fn index(&self, k: &K) -> &V {
        self.get(k).expect("SortedMap: no entry for the key")
    }
}

impl<K, V> IntoIterator for SortedMap<K, V> {
    type Item = (K, V);
    type IntoIter = std::vec::IntoIter<(K, V)>;
    fn into_iter(self) -> Self::IntoIter {
        self.pairs.into_iter()
    }
}

impl<'a, K, V> IntoIterator for &'a SortedMap<K, V> {
    type Item = (&'a K, &'a V);
    type IntoIter = std::iter::Map<std::slice::Iter<'a, (K, V)>, fn(&'a (K, V)) -> (&'a K, &'a V)>;
    fn into_iter(self) -> Self::IntoIter {
        self.pairs.iter().map(|(k, v)| (k, v))
    }
}

impl<K: Ord> SortedSet<K> {
    /// `HashSet::with_capacity`'s shape.
    pub fn with_capacity(n: usize) -> Self {
        Self { items: Vec::with_capacity(n) }
    }

    pub fn drain(&mut self) -> std::vec::Drain<'_, K> {
        self.items.drain(..)
    }
}

impl<K> IntoIterator for SortedSet<K> {
    type Item = K;
    type IntoIter = std::vec::IntoIter<K>;
    fn into_iter(self) -> Self::IntoIter {
        self.items.into_iter()
    }
}

impl<'a, K> IntoIterator for &'a SortedSet<K> {
    type Item = &'a K;
    type IntoIter = std::slice::Iter<'a, K>;
    fn into_iter(self) -> Self::IntoIter {
        self.items.iter()
    }
}

impl<K: PartialEq> IntoIterator for LinearSet<K> {
    type Item = K;
    type IntoIter = std::vec::IntoIter<K>;
    fn into_iter(self) -> Self::IntoIter {
        self.items.into_iter()
    }
}

impl<K: PartialEq> FromIterator<K> for LinearSet<K> {
    fn from_iter<I: IntoIterator<Item = K>>(it: I) -> Self {
        let mut s = Self::new();
        for k in it {
            s.insert(k);
        }
        s
    }
}

impl<K: Ord, V> SortedMap<K, V> {
    /// A batch with the FIRST value for each key kept (`entry(k).or_insert(v)` over the batch in order), sorted once.
    pub fn from_iter_first<I: IntoIterator<Item = (K, V)>>(it: I) -> Self {
        let mut pairs: Vec<(K, V)> = it.into_iter().collect();
        // A stable sort keeps repeats in arrival order: the first of each run is the one kept.
        pairs.sort_by(|a, b| a.0.cmp(&b.0));
        pairs.dedup_by(|later, first| later.0 == first.0);
        Self { pairs }
    }
}
