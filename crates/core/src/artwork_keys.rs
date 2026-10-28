//! Bookkeeping for artwork requests: which (size, path) pairs are cached,
//! which are in flight, and which to drop when the cache is full.
//!
//! No UI types here on purpose: `app` wraps this around `slint::Image`, and
//! the logic stays testable on machines without libmpv or a display.

use std::collections::{HashSet, VecDeque};

/// Key of one requested image: the pixel size and the artwork path.
pub fn image_key(path: &str, px: u32) -> String {
    format!("{px}|{path}")
}

/// A bounded map of image keys with an in-flight set. Dropping an image
/// forgets its request, so scrolling back asks for it again.
pub struct KeyCache {
    /// Insertion order, oldest first.
    order: VecDeque<String>,
    /// Keys asked for and not answered yet.
    requested: HashSet<String>,
    limit: usize,
}

impl Default for KeyCache {
    fn default() -> Self {
        Self::new(96)
    }
}

impl KeyCache {
    /// A screen shows at most ~30 cards: 96 keeps a few screenfuls.
    pub const DEFAULT_LIMIT: usize = 96;

    pub fn new(limit: usize) -> Self {
        Self {
            order: VecDeque::new(),
            requested: HashSet::new(),
            limit: limit.max(1),
        }
    }

    /// `true` the first time a key is asked for. The caller then sends the
    /// request; the key stays claimed until [`KeyCache::store`] or
    /// [`KeyCache::clear`], so a repaint cannot ask twice (the core has no
    /// in-flight de-duplication of its own).
    pub fn claim(&mut self, key: &str) -> bool {
        self.requested.insert(key.to_owned())
    }

    /// Records an answer and returns the keys evicted to stay under the
    /// limit (their claims are dropped with them).
    pub fn store(&mut self, key: &str) -> Vec<String> {
        self.requested.remove(key);
        if self.order.iter().any(|k| k == key) {
            return Vec::new();
        }
        self.order.push_back(key.to_owned());
        let mut evicted = Vec::new();
        while self.order.len() > self.limit {
            let Some(old) = self.order.pop_front() else {
                break;
            };
            self.requested.remove(&old);
            evicted.push(old);
        }
        evicted
    }

    /// The request failed: forget the claim so a later repaint can retry.
    pub fn release(&mut self, key: &str) {
        self.requested.remove(key);
    }

    pub fn is_cached(&self, key: &str) -> bool {
        self.order.iter().any(|k| k == key)
    }

    pub fn is_claimed(&self, key: &str) -> bool {
        self.requested.contains(key)
    }

    pub fn len(&self) -> usize {
        self.order.len()
    }

    pub fn is_empty(&self) -> bool {
        self.order.is_empty()
    }

    /// Forgets everything (a new view, a different decode size).
    pub fn clear(&mut self) {
        self.order.clear();
        self.requested.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_carry_the_pixel_size() {
        assert_eq!(image_key("/api/v1/artwork/1", 256), "256|/api/v1/artwork/1");
        // The same artwork at two sizes is two entries.
        assert_ne!(image_key("/a", 256), image_key("/a", 512));
    }

    #[test]
    fn every_size_can_be_stored() {
        // The UI asks for cards at the layout size, list thumbs at 64 and
        // header / now-playing covers at 256: none of them may be rejected.
        let mut cache = KeyCache::default();
        for px in [160, 64, 256] {
            let key = image_key("/a", px);
            assert!(cache.claim(&key));
            cache.store(&key);
            assert!(cache.is_cached(&key), "px {px} was not stored");
            assert!(!cache.is_claimed(&key), "px {px} stayed in flight");
        }
    }

    #[test]
    fn a_failed_request_can_be_asked_again() {
        let mut cache = KeyCache::default();
        let key = image_key("/a", 160);
        assert!(cache.claim(&key));
        // A failed load releases the claim; without that the cover would stay
        // blank for the rest of the view.
        cache.release(&key);
        assert!(cache.claim(&key));
    }

    #[test]
    fn a_key_is_claimed_once_until_answered() {
        let mut cache = KeyCache::default();
        let key = image_key("/a", 160);
        assert!(cache.claim(&key));
        assert!(!cache.claim(&key), "the same request must not repeat");
        assert!(
            cache.claim(&image_key("/a", 64)),
            "another size is another request"
        );
        cache.store(&key);
        // Answered, then dropped: it can be asked for again.
        cache.clear();
        assert!(cache.claim(&key));
    }

    #[test]
    fn the_oldest_entries_are_evicted_and_reloadable() {
        let mut cache = KeyCache::new(4);
        for i in 0..6 {
            let key = image_key(&format!("/a/{i}"), 64);
            cache.claim(&key);
            let evicted = cache.store(&key);
            assert_eq!(evicted.len(), usize::from(i >= 4));
        }
        assert!(!cache.is_cached(&image_key("/a/0", 64)));
        assert!(cache.is_cached(&image_key("/a/5", 64)));
        assert_eq!(cache.len(), 4);
        // An evicted key is claimable again, so a scroll back reloads it.
        assert!(cache.claim(&image_key("/a/0", 64)));
    }
}
