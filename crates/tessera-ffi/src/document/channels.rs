//! Persistent alpha and spot channels on a [`DocumentSession`] (WP B5-08):
//! the Channels panel, Select ▸ Save / Load Selection and Quick Mask.
//!
//! Channels live in the document (`DocState::channels`, compositor
//! `channels.rs`), so they are saved in `.tessera-doc` and PSD files and every
//! change is one `DocOp` (`AddChannel`, `DeleteChannel`, `RenameChannel`,
//! `EditChannel`, or `SetSelection` for loading) and therefore one history
//! node that undo and redo restore.
//!
//! Spot colour and solidity, and alpha overlay colour, opacity and
//! masked / selected indicator (B5-17b), are saved **preview metadata only**: they never change
//! the RGB composite or flat export. Channel visibility (the panel's eye) is
//! session state for the host's preview overlay and is not saved.
//!
//! The legacy name-based calls in `tools.rs` (`save_selection`,
//! `load_selection`, `selection_channels`) delegate here; names may repeat, the
//! id-based calls below tell such channels apart.

use super::{DocumentSession, DocumentUpdate, PaintColor, SelectionOp, Shared, io};
use crate::{Result, failure, surface::Surface};
use compositor::{
    DocOp, DocState, Raster,
    channels::{ChannelId, ChannelKind, DocumentChannel},
    document::selection::{self as docsel, Combine},
};
use engine_api::tile::{TILE_SIZE, TileCoord};
use std::{
    collections::{BTreeSet, HashMap},
    sync::{Arc, Mutex, Weak},
};

// ─────────────────────────────── records ───────────────────────────────

/// What a saved channel is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum DocChannelKind {
    /// A saved selection or general-purpose mask.
    Alpha,
    /// A spot ink plane (preview metadata only: never in the RGB composite).
    Spot,
}

/// One saved channel, as the Channels panel lists it.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct ChannelRecord {
    /// Stable within the document (and across save / reopen).
    pub id: u64,
    pub kind: DocChannelKind,
    /// Display name; names may repeat (as in PSD).
    pub name: String,
    /// Spot: the ink's display colour. Alpha: the overlay colour (red by
    /// default; saved in `.tessera-doc` and PSD, set by
    /// `set_alpha_channel_display`).
    pub color: PaintColor,
    /// Spot: solidity. Alpha: the overlay opacity (0.5 by default).
    pub opacity: f32,
    // M5-32: alpha overlay polarity; spot ink is identified by `kind`.
    pub selected_areas: bool,
    /// Shown by the host's preview overlay (session state, not saved).
    pub visible: bool,
    /// Position in the panel and in the PSD channel list (0 = first).
    pub index: u32,
    /// Changes whenever the channel's samples change (thumbnail cache key).
    pub revision: u64,
}

/// A channel edit that names the channel it made or changed.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct ChannelUpdate {
    pub channel_id: u64,
    pub update: DocumentUpdate,
}

/// Default display metadata PSD writes for alpha channels.
const ALPHA_COLOR: [f32; 3] = [1.0, 0.0, 0.0];
const ALPHA_OPACITY: f32 = 0.5;
/// Sample versions remembered per channel for stable revisions.
const REVISIONS_KEPT: usize = 16;

// ─────────────────────────────── session side state ───────────────────────────────

/// Per-session preview state: visible channels, revisions and thumbnails.
/// Kept beside the session (keyed by its shared state) so `State` stays
/// B5-01's; entries of closed sessions are dropped on the next access.
#[derive(Default)]
struct Side {
    visible: BTreeSet<u64>,
    /// Channel id → recent (samples seen, revision number given to them),
    /// newest last, so undo and redo find their earlier revision again.
    revisions: HashMap<u64, Vec<(Raster, u64)>>,
    next_revision: u64,
    /// (channel id, max_px) → (samples rendered, surface).
    thumbs: HashMap<(u64, u32), (Raster, Arc<Surface>)>,
}

type SideTable = Vec<(Weak<Shared>, Side)>;

fn side_table() -> &'static Mutex<SideTable> {
    static TABLE: std::sync::OnceLock<Mutex<SideTable>> = std::sync::OnceLock::new();
    TABLE.get_or_init(Default::default)
}

fn with_side<R>(shared: &Arc<Shared>, f: impl FnOnce(&mut Side) -> R) -> R {
    let mut t = side_table().lock().unwrap_or_else(|e| e.into_inner());
    t.retain(|(w, _)| w.strong_count() > 0);
    let i = match t
        .iter()
        .position(|(w, _)| std::ptr::eq(w.as_ptr(), Arc::as_ptr(shared)))
    {
        Some(i) => i,
        None => {
            t.push((Arc::downgrade(shared), Side::default()));
            t.len() - 1
        }
    };
    f(&mut t[i].1)
}

/// The same samples (shared tiles and default), i.e. an unchanged channel.
fn same_samples(a: &Raster, b: &Raster) -> bool {
    a.extent() == b.extent()
        && a.depth() == b.depth()
        && a.default_value() == b.default_value()
        && a.shares_all_tiles_with(b)
        && b.shares_all_tiles_with(a)
}

// ─────────────────────────────── helpers ───────────────────────────────

fn channel(state: &DocState, id: u64) -> Result<&DocumentChannel> {
    state
        .channels
        .iter()
        .find(|c| c.id.0 == id)
        .ok_or_else(|| failure(format!("channel {id} not found")))
}

fn check_name(name: &str) -> Result<String> {
    let n = name.trim();
    if n.is_empty() {
        return Err(failure("channel name is empty"));
    }
    Ok(n.to_owned())
}

fn check_spot(color: PaintColor, solidity: f32) -> Result<ChannelKind> {
    let color = [color.r, color.g, color.b];
    if color
        .iter()
        .chain(std::iter::once(&solidity))
        .any(|v| !v.is_finite() || !(0.0..=1.0).contains(v))
    {
        return Err(failure(
            "spot colour and solidity must be finite values within 0…1",
        ));
    }
    Ok(ChannelKind::Spot { color, solidity })
}

/// Alpha display metadata (B5-17b): `Alpha` for the legacy red / 50 % /
/// masked-areas default (so untouched and reset channels keep their legacy
/// identity), otherwise `AlphaDisplay`.
fn check_alpha_display(color: PaintColor, opacity: f32, selected: bool) -> Result<ChannelKind> {
    let color = [color.r, color.g, color.b];
    if color
        .iter()
        .chain(std::iter::once(&opacity))
        .any(|v| !v.is_finite() || !(0.0..=1.0).contains(v))
    {
        return Err(failure(
            "alpha overlay colour and opacity must be finite values within 0…1",
        ));
    }
    Ok(
        if color == ALPHA_COLOR && opacity == ALPHA_OPACITY && !selected {
            ChannelKind::Alpha
        } else {
            ChannelKind::AlphaDisplay {
                color,
                opacity,
                selected,
            }
        },
    )
}

/// `r` as a selection: single-channel F32 (PSD channels arrive at the
/// document depth), optionally inverted.
fn as_selection(r: &Raster, invert: bool) -> Result<Raster> {
    let f = |v: f32| if invert { 1.0 - v } else { v };
    if r.depth() == compositor::Depth::F32 && !invert {
        return Ok(r.clone());
    }
    let mut out = Raster::new(r.extent(), 1, compositor::Depth::F32, f(r.default_value()));
    let (cols, rows) = r.grid();
    let mut buf = Vec::new();
    for ty in 0..rows {
        for tx in 0..cols {
            if r.tile(tx, ty).is_none() {
                continue;
            }
            r.read_tile(tx, ty, &mut buf).map_err(failure)?;
            let data: Vec<f32> = buf.iter().map(|v| f(*v)).collect();
            let t = engine_api::tile::Tile::from_samples(
                TileCoord::new(0, tx, ty),
                out.layout(tx, ty),
                data,
            )
            .map_err(failure)?;
            out.set_slot(tx, ty, Some(t), 0).map_err(failure)?;
        }
    }
    Ok(out)
}

/// An empty (`0`, all masked) or full (`1`, all selected) channel plane.
fn plane(state: &DocState, value: f32) -> Raster {
    Raster::new(state.canvas, 1, compositor::Depth::F32, value)
}

impl DocumentSession {
    /// The current (live) document state.
    fn channel_state(&self) -> Result<Arc<DocState>> {
        let st = self.shared.read()?;
        st.open()?;
        Ok(st.live().clone())
    }

    /// The live selection, or an error naming what needed it.
    fn selection_for(&self, what: &str) -> Result<Raster> {
        self.channel_state()?
            .selection
            .as_deref()
            .cloned()
            .ok_or_else(|| failure(format!("{what}: there is no selection")))
    }

    /// Adds `channel` (id allocated) as one history node; returns its id.
    fn add_channel(&self, channel: DocumentChannel, label: &str) -> Result<ChannelUpdate> {
        self.add_channel_with(channel, Vec::new(), label)
    }

    /// Adds `channel` (id allocated) and applies `more` after it, all as one
    /// history node; returns the new channel's id.
    fn add_channel_with(
        &self,
        channel: DocumentChannel,
        more: Vec<DocOp>,
        label: &str,
    ) -> Result<ChannelUpdate> {
        let before: BTreeSet<u64> = self
            .channel_state()?
            .channels
            .iter()
            .map(|c| c.id.0)
            .collect();
        let add = DocOp::AddChannel { channel };
        let op = if more.is_empty() {
            add
        } else {
            DocOp::Batch(std::iter::once(add).chain(more).collect())
        };
        let update = self.edit(op, Some(label))?;
        let id = self
            .channel_state()?
            .channels
            .iter()
            .map(|c| c.id.0)
            .find(|id| !before.contains(id))
            .ok_or_else(|| failure("channel was not added"))?;
        Ok(ChannelUpdate {
            channel_id: id,
            update,
        })
    }

    /// Replaces channel `id` through `f` as one history node.
    fn edit_channel(
        &self,
        id: u64,
        label: &str,
        f: impl FnOnce(&mut DocumentChannel) -> Result<()>,
    ) -> Result<DocumentUpdate> {
        let mut c = channel(&*self.channel_state()?, id)?.clone();
        f(&mut c)?;
        self.edit(DocOp::EditChannel { channel: c }, Some(label))
    }

    fn channel_revision(&self, c: &DocumentChannel) -> u64 {
        with_side(&self.shared, |side| {
            let seen = side.revisions.entry(c.id.0).or_default();
            if let Some((_, rev)) = seen.iter().find(|(r, _)| same_samples(r, &c.raster)) {
                return *rev;
            }
            side.next_revision += 1;
            let rev = side.next_revision;
            if seen.len() >= REVISIONS_KEPT {
                seen.remove(0);
            }
            seen.push((c.raster.clone(), rev));
            rev
        })
    }
}

// ─────────────────────────────── exported calls ───────────────────────────────

#[uniffi::export]
impl DocumentSession {
    /// Saved alpha and spot channels in panel / PSD order.
    pub fn document_channels(&self) -> Result<Vec<ChannelRecord>> {
        let state = self.channel_state()?;
        let visible = with_side(&self.shared, |s| {
            let live: BTreeSet<u64> = state.channels.iter().map(|c| c.id.0).collect();
            s.visible.retain(|id| live.contains(id));
            s.visible.clone()
        });
        Ok(state
            .channels
            .iter()
            .enumerate()
            .map(|(i, c)| {
                let (kind, color, opacity) = match &c.kind {
                    ChannelKind::Alpha => (DocChannelKind::Alpha, ALPHA_COLOR, ALPHA_OPACITY),
                    // M5-32: preserve explicit alpha preview properties.
                    ChannelKind::AlphaDisplay { color, opacity, .. } => {
                        (DocChannelKind::Alpha, *color, *opacity)
                    }
                    ChannelKind::Spot { color, solidity } => {
                        (DocChannelKind::Spot, *color, *solidity)
                    }
                };
                ChannelRecord {
                    id: c.id.0,
                    kind,
                    name: c.name.clone(),
                    color: PaintColor {
                        r: color[0],
                        g: color[1],
                        b: color[2],
                    },
                    opacity,
                    // M5-32: legacy alpha channels display masked areas.
                    selected_areas: matches!(
                        c.kind,
                        ChannelKind::AlphaDisplay { selected: true, .. }
                    ),
                    visible: visible.contains(&c.id.0),
                    index: i as u32,
                    revision: self.channel_revision(c),
                }
            })
            .collect())
    }

    /// Select ▸ Save Selection: the selection as a new alpha channel `name`
    /// (`target` `None`; `op` is then ignored), or combined by `op` into
    /// existing channel `target` (Replace, Add, Subtract, Intersect). One
    /// history node; fails without a selection.
    pub fn save_selection_channel(
        &self,
        name: String,
        target: Option<u64>,
        op: SelectionOp,
    ) -> Result<ChannelUpdate> {
        let sel = as_selection(&self.selection_for("Save Selection")?, false)?;
        match target {
            None => self.add_channel(
                DocumentChannel {
                    id: ChannelId(0),
                    name: check_name(&name)?,
                    kind: ChannelKind::Alpha,
                    raster: sel,
                },
                "Save Selection",
            ),
            Some(id) => {
                let update = self.edit_channel(id, "Save Selection", |c| {
                    let cur = as_selection(&c.raster, false)?;
                    let how = match op {
                        SelectionOp::Replace => Combine::Replace,
                        SelectionOp::Add => Combine::Add,
                        SelectionOp::Subtract => Combine::Subtract,
                        SelectionOp::Intersect => Combine::Intersect,
                    };
                    c.raster = docsel::combine(&cur, &sel, how).map_err(failure)?;
                    Ok(())
                })?;
                Ok(ChannelUpdate {
                    channel_id: id,
                    update,
                })
            }
        }
    }

    /// Select ▸ Load Selection: channel `id` (inverted when `invert`)
    /// combined with the current selection by `op`. One history node.
    pub fn load_selection_channel(
        &self,
        id: u64,
        op: SelectionOp,
        invert: bool,
    ) -> Result<DocumentUpdate> {
        let raster = as_selection(&channel(&*self.channel_state()?, id)?.raster, invert)?;
        self.apply_selection(Some(raster), op, "Load Selection")
    }

    /// A new empty alpha channel: all masked (black), or all selected
    /// (white) when `selected` (Quick Mask without a selection).
    pub fn new_alpha_channel(&self, name: String, selected: bool) -> Result<ChannelUpdate> {
        let state = self.channel_state()?;
        self.add_channel(
            DocumentChannel {
                id: ChannelId(0),
                name: check_name(&name)?,
                kind: ChannelKind::Alpha,
                raster: plane(&state, if selected { 1.0 } else { 0.0 }),
            },
            "New Channel",
        )
    }

    pub fn rename_document_channel(&self, id: u64, name: String) -> Result<DocumentUpdate> {
        let name = check_name(&name)?;
        channel(&*self.channel_state()?, id)?;
        self.edit(
            DocOp::RenameChannel {
                id: ChannelId(id),
                name,
            },
            Some("Rename Channel"),
        )
    }

    pub fn delete_document_channel(&self, id: u64) -> Result<DocumentUpdate> {
        channel(&*self.channel_state()?, id)?;
        let update = self.edit(
            DocOp::DeleteChannel { id: ChannelId(id) },
            Some("Delete Channel"),
        )?;
        with_side(&self.shared, |s| {
            s.visible.remove(&id);
        });
        Ok(update)
    }

    /// A copy of channel `id` named "<name> copy", appended last.
    pub fn duplicate_document_channel(&self, id: u64) -> Result<ChannelUpdate> {
        let mut c = channel(&*self.channel_state()?, id)?.clone();
        c.id = ChannelId(0);
        c.name = format!("{} copy", c.name);
        self.add_channel(c, "Duplicate Channel")
    }

    /// Channel Options for a spot channel (an alpha channel becomes one):
    /// display colour and solidity, each within 0…1. Preview metadata only.
    pub fn set_spot_channel(
        &self,
        id: u64,
        color: PaintColor,
        solidity: f32,
    ) -> Result<DocumentUpdate> {
        let kind = check_spot(color, solidity)?;
        self.edit_channel(id, "Channel Options", |c| {
            c.kind = kind;
            Ok(())
        })
    }

    /// Channel Options for an alpha channel (a spot channel becomes one):
    /// overlay colour and opacity, each within 0…1, and whether the colour
    /// marks selected (`selected_areas`) or masked areas. One "Channel
    /// Options" history node; saved in `.tessera-doc` and PSD. Preview
    /// metadata only: the samples and the RGB composite are unchanged.
    pub fn set_alpha_channel_display(
        &self,
        id: u64,
        color: PaintColor,
        opacity: f32,
        selected_areas: bool,
    ) -> Result<DocumentUpdate> {
        let kind = check_alpha_display(color, opacity, selected_areas)?;
        self.edit_channel(id, "Channel Options", |c| {
            c.kind = kind;
            Ok(())
        })
    }

    /// New Spot Channel: an ink plane from the selection (`from_selection`)
    /// or empty. Preview metadata only: the RGB composite is unchanged.
    pub fn new_spot_channel(
        &self,
        name: String,
        color: PaintColor,
        solidity: f32,
        from_selection: bool,
    ) -> Result<ChannelUpdate> {
        let kind = check_spot(color, solidity)?;
        let name = check_name(&name)?;
        let raster = if from_selection {
            as_selection(&self.selection_for("New Spot Channel")?, false)?
        } else {
            plane(&*self.channel_state()?, 0.0)
        };
        self.add_channel(
            DocumentChannel {
                id: ChannelId(0),
                name,
                kind,
                raster,
            },
            "New Spot Channel",
        )
    }

    /// Quick Mask on (B5-17d): the selection becomes the new, visible alpha
    /// channel `name` and the selection is dropped, so strokes into the mask
    /// are not clipped to it and painting white can grow it. Without a
    /// selection the channel is all selected (white). One "Quick Mask"
    /// history node; undo restores the selection.
    pub fn enter_quick_mask(&self, name: String) -> Result<ChannelUpdate> {
        let name = check_name(&name)?;
        let state = self.channel_state()?;
        let (raster, more) = match state.selection.as_deref() {
            Some(sel) => (
                as_selection(sel, false)?,
                vec![DocOp::SetSelection { selection: None }],
            ),
            None => (plane(&state, 1.0), Vec::new()),
        };
        let made = self.add_channel_with(
            DocumentChannel {
                id: ChannelId(0),
                name,
                kind: ChannelKind::Alpha,
                raster,
            },
            more,
            "Quick Mask",
        )?;
        with_side(&self.shared, |s| {
            s.visible.insert(made.channel_id);
        });
        Ok(made)
    }

    /// Quick Mask off (B5-17d): channel `id` replaces the selection (an
    /// empty mask deselects) and is deleted, as one "Quick Mask" history
    /// node. Fails without a node when `id` is unknown.
    pub fn exit_quick_mask(&self, id: u64) -> Result<DocumentUpdate> {
        let raster = as_selection(&channel(&*self.channel_state()?, id)?.raster, false)?;
        let selection = Some(raster).filter(|r| io::selection_bounds(r).is_some());
        let update = self.edit(
            DocOp::Batch(vec![
                DocOp::SetSelection { selection },
                DocOp::DeleteChannel { id: ChannelId(id) },
            ]),
            Some("Quick Mask"),
        )?;
        with_side(&self.shared, |s| {
            s.visible.remove(&id);
        });
        Ok(update)
    }

    /// Shows or hides channel `id` in the host's preview overlay (session
    /// state: not saved, not a history node).
    pub fn set_channel_visible(&self, id: u64, visible: bool) -> Result<()> {
        channel(&*self.channel_state()?, id)?;
        with_side(&self.shared, |s| {
            if visible {
                s.visible.insert(id);
            } else {
                s.visible.remove(&id);
            }
        });
        Ok(())
    }

    /// Channel `id` as a grey RGBA8 IOSurface (white = selected / full ink),
    /// at most `max_px` on the long edge (box-filtered). Cached per channel
    /// samples: the id stays valid until a newer thumbnail of the same
    /// channel and size replaces it or the session closes.
    pub fn channel_thumbnail(&self, id: u64, max_px: u32) -> Result<u32> {
        if max_px == 0 || max_px > 4096 {
            return Err(failure("max_px must be 1…4096"));
        }
        let raster = channel(&*self.channel_state()?, id)?.raster.clone();
        if let Some(sid) = with_side(&self.shared, |s| {
            s.thumbs
                .get(&(id, max_px))
                .filter(|(seen, _)| same_samples(seen, &raster))
                .map(|(_, surface)| surface.id())
        }) {
            return Ok(sid);
        }
        let e = raster.extent();
        let long = e.width.max(e.height).max(1);
        let step = long.div_ceil(max_px).max(1);
        let (w, h) = (
            e.width.div_ceil(step).max(1),
            e.height.div_ceil(step).max(1),
        );
        let mut sum = vec![0f32; (w * h) as usize];
        let mut count = vec![0u32; (w * h) as usize];
        let (cols, rows) = raster.grid();
        let mut buf = Vec::new();
        for ty in 0..rows {
            for tx in 0..cols {
                let l = raster.layout(tx, ty);
                let (ox, oy) = (tx * TILE_SIZE, ty * TILE_SIZE);
                raster.read_tile(tx, ty, &mut buf).map_err(failure)?;
                for y in 0..l.extent.height {
                    let row = ((oy + y) / step * w) as usize;
                    for x in 0..l.extent.width {
                        let o = row + ((ox + x) / step) as usize;
                        sum[o] += buf[(y * l.extent.width + x) as usize];
                        count[o] += 1;
                    }
                }
            }
        }
        let surface = Surface::create_rgba8(w, h).map_err(failure)?;
        surface
            .with_pixels(|px, stride| {
                for y in 0..h as usize {
                    for x in 0..w as usize {
                        let i = y * w as usize + x;
                        let v = if count[i] == 0 {
                            0.0
                        } else {
                            sum[i] / count[i] as f32
                        };
                        let q = (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
                        let o = y * stride + x * 4;
                        px[o..o + 4].copy_from_slice(&[q, q, q, 255]);
                    }
                }
            })
            .map_err(failure)?;
        let sid = surface.id();
        with_side(&self.shared, |s| {
            s.thumbs.insert((id, max_px), (raster, Arc::new(surface)));
        });
        Ok(sid)
    }
}

// ─────────────────────────────── legacy name-based calls ───────────────────────────────

impl DocumentSession {
    /// `save_selection(name)`: replaces the first alpha channel named `name`
    /// or adds one.
    pub(super) fn legacy_save_selection(&self, name: String) -> Result<()> {
        let name = check_name(&name)?;
        let existing = self
            .channel_state()?
            .channels
            .iter()
            // M5-32: explicit display metadata does not change alpha identity.
            .find(|c| {
                c.name == name
                    && matches!(
                        c.kind,
                        ChannelKind::Alpha | ChannelKind::AlphaDisplay { .. }
                    )
            })
            .map(|c| c.id.0);
        self.save_selection_channel(name, existing, SelectionOp::Replace)
            .map(|_| ())
    }

    /// `load_selection(name, op)`: the first channel named `name`.
    pub(super) fn legacy_load_selection(
        &self,
        name: String,
        op: SelectionOp,
    ) -> Result<DocumentUpdate> {
        let id = self
            .channel_state()?
            .channels
            .iter()
            .find(|c| c.name == name)
            .map(|c| c.id.0)
            .ok_or_else(|| failure(format!("no channel named {name:?}")))?;
        self.load_selection_channel(id, op, false)
    }

    /// `selection_channels()`: every channel's name in panel order
    /// (duplicates included; `document_channels` tells them apart by id).
    pub(super) fn legacy_selection_channels(&self) -> Result<Vec<String>> {
        Ok(self
            .channel_state()?
            .channels
            .iter()
            .map(|c| c.name.clone())
            .collect())
    }
}
