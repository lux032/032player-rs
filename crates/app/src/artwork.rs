//! Decoded artwork kept on the UI thread. The core caches compressed bytes
//! and decodes to the requested pixel size, so the UI only holds a few dozen
//! ready images and drops the oldest when scrolling far.
//!
//! The bookkeeping (which keys are cached, in flight, or evicted) lives in
//! `client_core::artwork_keys` so it can be tested without a display.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;

use client_core::artwork_keys::{KeyCache, image_key};
use slint::Image;

/// The pixel sizes the pane asks for: list thumbs and header / now-playing
/// covers. Card covers use the layout size instead.
pub const THUMB_PX: u32 = 64;
pub const HEADER_PX: u32 = 256;

#[derive(Default)]
pub struct ArtworkCache {
    images: RefCell<HashMap<String, Image>>,
    keys: RefCell<KeyCache>,
    /// Pixel size the cards are decoded at (card size x scale factor).
    px: Cell<u32>,
}

impl ArtworkCache {
    /// The decoded image of `path` at `px`.
    pub fn image(&self, path: &str, px: u32) -> Option<Image> {
        if path.is_empty() {
            return None;
        }
        self.images.borrow().get(&image_key(path, px)).cloned()
    }

    pub fn cover_px(&self) -> u32 {
        self.px.get().max(64)
    }

    /// Sets the card decode size; true when it changed enough to reload
    /// (over 125% growth looks blurry, under 60% wastes memory).
    pub fn set_cover_px(&self, px: u32) -> bool {
        let px = px.clamp(64, 1536);
        let current = self.cover_px();
        let reload = px * 4 > current * 5 || px * 5 < current * 3;
        if reload {
            self.px.set(px);
            self.reset();
        }
        reload
    }

    /// `true` the first time a key is asked for: the caller then sends
    /// `Command::LoadImage`. The core has no in-flight de-duplication, so an
    /// unclaimed request would repeat on every repaint.
    pub fn claim(&self, path: &str, px: u32) -> bool {
        if path.is_empty() {
            return false;
        }
        self.keys.borrow_mut().claim(&image_key(path, px))
    }

    /// Stores a decoded image.
    pub fn store(&self, key: &str, image: Image) {
        let evicted = self.keys.borrow_mut().store(key);
        let mut images = self.images.borrow_mut();
        for key in evicted {
            images.remove(&key);
        }
        images.insert(key.to_owned(), image);
    }

    /// The load failed: let a later repaint ask again.
    pub fn release(&self, key: &str) {
        self.keys.borrow_mut().release(key);
    }

    pub fn reset(&self) {
        self.images.borrow_mut().clear();
        self.keys.borrow_mut().clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_carry_the_pixel_size() {
        assert_eq!(image_key("/api/v1/artwork/1", 256), "256|/api/v1/artwork/1");
        assert_ne!(image_key("/a", 256), image_key("/a", 512));
    }

    #[test]
    fn every_size_the_pane_uses_round_trips() {
        let cache = ArtworkCache::default();
        cache.px.set(160);
        // Card covers, list thumbs and header covers must all be storable.
        for px in [160, THUMB_PX, HEADER_PX] {
            assert!(cache.claim("/a", px), "px {px} was already claimed");
            cache.store(&image_key("/a", px), Image::default());
            assert!(cache.image("/a", px).is_some(), "px {px} was not stored");
        }
    }

    #[test]
    fn a_resize_forgets_the_images() {
        let cache = ArtworkCache::default();
        cache.px.set(256);
        cache.claim("/a", 256);
        cache.store(&image_key("/a", 256), Image::default());
        assert!(cache.image("/a", 256).is_some());
        assert!(!cache.set_cover_px(280), "a small resize keeps the images");
        assert!(cache.set_cover_px(400), "growing past 125% reloads");
        assert!(cache.image("/a", 256).is_none());
        assert!(cache.claim("/a", 256), "the claims are dropped too");
    }
}
