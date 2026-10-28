//! Row models of the main pane. Slint asks a `Model` for the rows it is about
//! to draw, so building a row there means only visible rows cost anything -
//! in particular only visible rows request artwork. Requesting while
//! rebuilding *every* loaded row (the previous approach) loops once the
//! artwork cache is smaller than the list: an evicted cover is asked for
//! again on the next repaint, evicting another one.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use client_core::artwork_keys::image_key;
use client_core::view::{chunk_delta, chunk_indexes};
use client_core::{Commander, ViewRow};
use slint::{Image, Model, ModelNotify, ModelRc, ModelTracker, VecModel};

use crate::artwork::{ArtworkCache, THUMB_PX};
use crate::{Command, ViewRow as UiRow, ViewRowChunk};

/// The rows behind both models, and what building one row needs.
pub struct Rows {
    /// All loaded rows of the current view.
    pub data: RefCell<Vec<ViewRow>>,
    /// Row indexes by artwork path.
    by_artwork: RefCell<HashMap<String, Vec<usize>>>,
    /// Cards per grid row.
    pub columns: Cell<usize>,
    artwork: Rc<ArtworkCache>,
    commander: Commander,
}

impl Rows {
    pub fn new(artwork: Rc<ArtworkCache>, commander: Commander) -> Rc<Self> {
        Rc::new(Self {
            data: RefCell::default(),
            by_artwork: RefCell::default(),
            columns: Cell::new(4),
            artwork,
            commander,
        })
    }

    /// Asks for artwork, once per (size, path). The core has no in-flight
    /// de-duplication, so a repeated ask would loop.
    fn request(&self, path: &str, px: u32) {
        if !self.artwork.claim(path, px) {
            return;
        }
        self.commander.send(Command::LoadImage {
            key: image_key(path, px),
            path: path.to_owned(),
            px,
        });
    }

    fn cover(&self, path: &str, px: u32) -> Image {
        match self.artwork.image(path, px) {
            Some(image) => image,
            None => {
                self.request(path, px);
                Image::default()
            }
        }
    }

    /// Replaces the loaded rows (a new view).
    pub fn clear(&self) {
        self.data.borrow_mut().clear();
        self.by_artwork.borrow_mut().clear();
    }

    /// Appends rows and returns how many were added.
    pub fn append(&self, rows: Vec<ViewRow>) -> usize {
        let mut data = self.data.borrow_mut();
        let mut by_artwork = self.by_artwork.borrow_mut();
        let start = data.len();
        for (offset, row) in rows.into_iter().enumerate() {
            if !row.artwork.is_empty() {
                by_artwork
                    .entry(row.artwork.clone())
                    .or_default()
                    .push(start + offset);
            }
            data.push(row);
        }
        data.len() - start
    }

    /// Grid rows (album cards).
    pub fn chunk_count(&self) -> usize {
        let columns = self.columns.get().max(1);
        self.data.borrow().len().div_ceil(columns)
    }

    pub fn chunk(&self, chunk: usize) -> ViewRowChunk {
        let columns = self.columns.get().max(1);
        let data = self.data.borrow();
        let start = chunk * columns;
        let end = (start + columns).min(data.len());
        let px = self.artwork.cover_px();
        let items: Vec<UiRow> = data[start.min(data.len())..end]
            .iter()
            .enumerate()
            .map(|(i, row)| self.card(start + i, row, px))
            .collect();
        ViewRowChunk {
            items: ModelRc::new(VecModel::from(items)),
        }
    }

    /// One list row.
    pub fn line(&self, index: usize) -> Option<UiRow> {
        let row = self.data.borrow().get(index)?.clone();
        Some(self.card(index, &row, THUMB_PX))
    }

    fn card(&self, index: usize, row: &ViewRow, px: u32) -> UiRow {
        UiRow {
            kind: kind_index(row.kind),
            id: int(row.id),
            index: i32::try_from(index).unwrap_or(i32::MAX),
            title: row.title.clone().into(),
            subtitle: row.subtitle.clone().into(),
            meta: row.meta.clone().into(),
            artwork: row.artwork.clone().into(),
            favorite: row.favorite,
            playable: row.play.is_some(),
            cover: if row.artwork.is_empty() {
                Image::default()
            } else {
                self.cover(&row.artwork, px)
            },
        }
    }

    /// The rows showing `path` (for a targeted repaint after an image).
    /// The index is maintained as rows are appended, so an image arriving
    /// does not walk a ten-thousand-row list.
    pub fn indexes_of(&self, path: &str) -> Vec<usize> {
        self.by_artwork
            .borrow()
            .get(path)
            .cloned()
            .unwrap_or_default()
    }

    pub fn artwork(&self) -> &Rc<ArtworkCache> {
        &self.artwork
    }
}

pub fn kind_index(kind: client_core::RowKind) -> i32 {
    match kind {
        client_core::RowKind::Album => 0,
        client_core::RowKind::Track => 1,
        client_core::RowKind::Artist => 2,
        client_core::RowKind::Playlist => 3,
    }
}

fn int(value: i64) -> i32 {
    i32::try_from(value).unwrap_or(if value < 0 { i32::MIN } else { i32::MAX })
}

/// The grid model: rows of album cards, built on demand.
pub struct GridModel {
    rows: Rc<Rows>,
    notify: ModelNotify,
}

impl GridModel {
    pub fn new(rows: Rc<Rows>) -> Rc<Self> {
        Rc::new(Self {
            rows,
            notify: ModelNotify::default(),
        })
    }
}

impl Model for GridModel {
    type Data = ViewRowChunk;

    fn row_count(&self) -> usize {
        self.rows.chunk_count()
    }

    fn row_data(&self, row: usize) -> Option<ViewRowChunk> {
        (row < self.row_count()).then(|| self.rows.chunk(row))
    }

    fn model_tracker(&self) -> &dyn ModelTracker {
        &self.notify
    }
}

/// The list model: one text row each.
pub struct ListModel {
    rows: Rc<Rows>,
    notify: ModelNotify,
}

impl ListModel {
    pub fn new(rows: Rc<Rows>) -> Rc<Self> {
        Rc::new(Self {
            rows,
            notify: ModelNotify::default(),
        })
    }
}

impl Model for ListModel {
    type Data = UiRow;

    fn row_count(&self) -> usize {
        self.rows.data.borrow().len()
    }

    fn row_data(&self, row: usize) -> Option<UiRow> {
        self.rows.line(row)
    }

    fn model_tracker(&self) -> &dyn ModelTracker {
        &self.notify
    }
}

/// Both models of the pane, so the UI can refresh them consistently.
pub struct Pane {
    pub rows: Rc<Rows>,
    pub grid: Rc<GridModel>,
    pub list: Rc<ListModel>,
}

impl Pane {
    pub fn new(rows: Rc<Rows>) -> Self {
        Self {
            grid: GridModel::new(rows.clone()),
            list: ListModel::new(rows.clone()),
            rows,
        }
    }

    /// Re-reads every row of the active layout (a layout or size change).
    pub fn reset(&self, grid: bool) {
        if grid {
            self.grid.notify.reset();
        } else {
            self.list.notify.reset();
        }
    }

    /// The rows grew by `added`.
    pub fn added(&self, grid: bool, before: usize, added: usize) {
        if added == 0 {
            return;
        }
        if grid {
            let delta = chunk_delta(before, added, self.rows.columns.get());
            if let Some(chunk) = delta.changed {
                self.grid.notify.row_changed(chunk);
            }
            if delta.added > 0 {
                self.grid.notify.row_added(delta.added_from, delta.added);
            }
        } else {
            self.list.notify.row_added(before, added);
        }
    }

    /// Some rows changed (after an image arrived or a favorite flipped).
    pub fn changed(&self, grid: bool, indexes: &[usize]) {
        if indexes.is_empty() {
            return;
        }
        if grid {
            for chunk in chunk_indexes(indexes, self.rows.columns.get()) {
                self.grid.notify.row_changed(chunk);
            }
        } else {
            for index in indexes {
                self.list.notify.row_changed(*index);
            }
        }
    }

    pub fn models(&self) -> (ModelRc<ViewRowChunk>, ModelRc<UiRow>) {
        (
            ModelRc::from(self.grid.clone()),
            ModelRc::from(self.list.clone()),
        )
    }
}
