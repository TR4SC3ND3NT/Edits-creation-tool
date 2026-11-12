//! A small least-recently-used cache of rendered preview frames.
//!
//! A frame is a pure function of the project and the frame number, so a
//! picture stays valid for as long as the project does: the key carries the
//! project revision, and a reload simply starts a new revision.

use std::collections::{BTreeMap, HashMap};

/// Preview size relative to the project's resolution.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Scale {
    Quarter,
    Half,
    Full,
}

impl Scale {
    pub const ALL: [Scale; 3] = [Scale::Quarter, Scale::Half, Scale::Full];

    pub fn factor(self) -> f32 {
        match self {
            Scale::Quarter => 0.25,
            Scale::Half => 0.5,
            Scale::Full => 1.0,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Scale::Quarter => "0.25",
            Scale::Half => "0.5",
            Scale::Full => "1.0",
        }
    }

    /// `0.25`, `0.5`, `1` (also `1/4`, `1/2`, `quarter`, `half`, `full`).
    pub fn parse(text: &str) -> Option<Self> {
        match text.trim() {
            "0.25" | ".25" | "1/4" | "quarter" => Some(Scale::Quarter),
            "0.5" | ".5" | "1/2" | "half" => Some(Scale::Half),
            "1" | "1.0" | "full" => Some(Scale::Full),
            _ => None,
        }
    }
}

/// What identifies a rendered picture.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FrameKey {
    /// Counts accepted versions of the project; a reload bumps it.
    pub revision: u64,
    pub frame: u64,
    pub scale: Scale,
    /// Rendered with the project's motion blur (false: one sample).
    pub blur: bool,
}

struct Entry<V> {
    value: V,
    bytes: usize,
    used: u64,
}

/// Holds values up to a byte budget and forgets the least recently used.
pub struct FrameCache<V> {
    map: HashMap<FrameKey, Entry<V>>,
    /// Use stamp → key, oldest first.
    order: BTreeMap<u64, FrameKey>,
    stamp: u64,
    bytes: usize,
    budget: usize,
}

impl<V> FrameCache<V> {
    pub fn new(budget_bytes: usize) -> Self {
        Self { map: HashMap::new(), order: BTreeMap::new(), stamp: 0, bytes: 0, budget: budget_bytes }
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    pub fn bytes(&self) -> usize {
        self.bytes
    }

    pub fn budget(&self) -> usize {
        self.budget
    }

    /// Is the picture there? Does not count as a use.
    pub fn contains(&self, key: &FrameKey) -> bool {
        self.map.contains_key(key)
    }

    /// Fetch a picture and mark it as just used.
    pub fn get(&mut self, key: &FrameKey) -> Option<&V> {
        let entry = self.map.get_mut(key)?;
        self.order.remove(&entry.used);
        self.stamp += 1;
        entry.used = self.stamp;
        self.order.insert(self.stamp, *key);
        Some(&entry.value)
    }

    /// Store a picture of `bytes` bytes, evicting the least recently used
    /// ones until the budget holds. The picture just stored always stays,
    /// even when it alone is over the budget.
    pub fn insert(&mut self, key: FrameKey, value: V, bytes: usize) {
        self.remove(&key);
        self.stamp += 1;
        self.order.insert(self.stamp, key);
        self.map.insert(key, Entry { value, bytes, used: self.stamp });
        self.bytes += bytes;
        while self.bytes > self.budget && self.map.len() > 1 {
            let Some((_, oldest)) = self.order.iter().next().map(|(s, k)| (*s, *k)) else { break };
            self.remove(&oldest);
        }
    }

    pub fn remove(&mut self, key: &FrameKey) -> Option<V> {
        let entry = self.map.remove(key)?;
        self.order.remove(&entry.used);
        self.bytes -= entry.bytes;
        Some(entry.value)
    }

    pub fn clear(&mut self) {
        self.map.clear();
        self.order.clear();
        self.bytes = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(revision: u64, frame: u64) -> FrameKey {
        FrameKey { revision, frame, scale: Scale::Half, blur: false }
    }

    #[test]
    fn keys_tell_revision_frame_scale_and_blur_apart() {
        let mut c = FrameCache::new(1000);
        let base = key(1, 5);
        c.insert(base, "base", 10);
        c.insert(FrameKey { revision: 2, ..base }, "new revision", 10);
        c.insert(FrameKey { frame: 6, ..base }, "next frame", 10);
        c.insert(FrameKey { scale: Scale::Full, ..base }, "full size", 10);
        c.insert(FrameKey { blur: true, ..base }, "blurred", 10);
        assert_eq!(c.len(), 5);
        assert_eq!(c.get(&base), Some(&"base"));
        assert_eq!(c.get(&FrameKey { blur: true, ..base }), Some(&"blurred"));
        assert_eq!(c.get(&FrameKey { scale: Scale::Quarter, ..base }), None);
        assert_eq!(c.bytes(), 50);
    }

    #[test]
    fn evicts_the_least_recently_used() {
        let mut c = FrameCache::new(30);
        c.insert(key(1, 0), 0, 10);
        c.insert(key(1, 1), 1, 10);
        c.insert(key(1, 2), 2, 10);
        // Touch frame 0: frame 1 is now the oldest.
        assert!(c.get(&key(1, 0)).is_some());
        c.insert(key(1, 3), 3, 10);
        assert!(c.contains(&key(1, 0)));
        assert!(!c.contains(&key(1, 1)));
        assert!(c.contains(&key(1, 2)));
        assert!(c.contains(&key(1, 3)));
        assert_eq!(c.bytes(), 30);
    }

    #[test]
    fn contains_does_not_count_as_a_use() {
        let mut c = FrameCache::new(20);
        c.insert(key(1, 0), 0, 10);
        c.insert(key(1, 1), 1, 10);
        assert!(c.contains(&key(1, 0)));
        c.insert(key(1, 2), 2, 10);
        assert!(!c.contains(&key(1, 0)), "frame 0 was only looked at, not used");
    }

    #[test]
    fn a_big_picture_evicts_several_and_always_stays() {
        let mut c = FrameCache::new(30);
        for f in 0..3 {
            c.insert(key(1, f), f, 10);
        }
        c.insert(key(1, 9), 9, 25);
        assert_eq!(c.len(), 1);
        assert!(c.contains(&key(1, 9)));
        c.insert(key(1, 10), 10, 500);
        assert_eq!(c.len(), 1, "over budget on its own, but the newest picture is kept");
        assert!(c.contains(&key(1, 10)));
        assert_eq!(c.bytes(), 500);
    }

    #[test]
    fn replacing_a_key_does_not_leak_bytes() {
        let mut c = FrameCache::new(100);
        c.insert(key(1, 0), "a", 40);
        c.insert(key(1, 0), "b", 30);
        assert_eq!((c.len(), c.bytes()), (1, 30));
        assert_eq!(c.get(&key(1, 0)), Some(&"b"));
        assert_eq!(c.remove(&key(1, 0)), Some("b"));
        assert_eq!((c.len(), c.bytes()), (0, 0));
        assert!(c.is_empty());
    }

    #[test]
    fn clear_forgets_everything() {
        let mut c = FrameCache::new(100);
        for f in 0..5 {
            c.insert(key(1, f), f, 10);
        }
        c.clear();
        assert_eq!((c.len(), c.bytes()), (0, 0));
        // And it keeps working afterwards.
        c.insert(key(2, 0), 0, 10);
        assert!(c.contains(&key(2, 0)));
    }

    #[test]
    fn scale_parses_and_prints() {
        for s in Scale::ALL {
            assert_eq!(Scale::parse(s.label()), Some(s));
        }
        assert_eq!(Scale::parse("1"), Some(Scale::Full));
        assert_eq!(Scale::parse("half"), Some(Scale::Half));
        assert_eq!(Scale::parse("0.3"), None);
        assert_eq!(Scale::Quarter.factor(), 0.25);
    }
}
