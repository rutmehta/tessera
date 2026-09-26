//! Document ops, non-linear history and the damage log.
//!
//! Every mutation is a [`DocOp`] applied by [`Document::apply`]; it gets one
//! fresh revision and produces one history node holding a structurally
//! shared [`DocState`] snapshot (unchanged layers and tiles are `Arc`
//! clones). History is a tree: undo/redo/checkout move `current`, and an op
//! applied after an undo starts a branch.

use std::collections::{BTreeMap, VecDeque};
use std::sync::Arc;

use engine_api::tile::{TILE_SIZE, Tile};
use engine_api::{EngineError, EngineResult};

use crate::adjust::Adjustment;
use crate::document::{DocState, Fill, GroupMode, Layer, LayerId, LayerKind, LayerProps, Mask};
use crate::geom::{Affine, Rect, next_doc_key, next_rev};
use crate::raster::{Raster, buffer_addr};

/// Which raster of a layer a paint op writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaintTarget {
    /// The layer's pixels (or text proxy).
    Content,
    /// The layer mask.
    Mask,
}

/// One replaced tile of a paint op (`None` erases to the default value).
#[derive(Debug, Clone)]
pub struct TileDelta {
    /// Tile column.
    pub tx: u32,
    /// Tile row.
    pub ty: u32,
    /// New samples.
    pub tile: Option<Tile>,
}

/// Host adapter for `filters::caf::fill` (filters already depends on compositor).
/// Receives straight RGBA, a union-hole mask and the deterministic seed. Return
/// a same-size RGBA composite; only hole pixels are installed in a new layer.
pub type ContentAwareFill = fn(&Raster, &[f32], u64) -> EngineResult<Raster>;

/// A document mutation — the unit of history.
// Ops are transient (history stores states, not ops), so the inline
// `Layer` in `AddLayer` is not worth a box in every caller.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone)]
pub enum DocOp {
    /// Register root pixel layers, retain their sources and extend the canvas.
    AutoAlignLayers {
        /// Ordered unique root layer IDs. First is the default reference.
        ids: Vec<LayerId>,
        /// Registration parameters.
        options: merge::layers::AlignOptions,
    },
    /// Compute editable panorama/focus masks in document coordinates.
    AutoBlendLayers {
        /// Ordered unique root layer IDs.
        ids: Vec<LayerId>,
        /// Seam, focus and fill parameters.
        options: merge::layers::BlendOptions,
        /// Required when content-aware filling is requested.
        fill: Option<ContentAwareFill>,
    },
    /// Open caller-decoded RGB images, align and blend in one atomic history node.
    Photomerge {
        /// Display names and scene-linear RGB images in overlap order.
        images: Vec<(String, merge::LinearImage)>,
        /// Alignment parameters.
        align: merge::layers::AlignOptions,
        /// Blending parameters.
        blend: merge::layers::BlendOptions,
        /// Adapter to `filters::caf::fill`, needed only for requested fill.
        fill: Option<ContentAwareFill>,
    },
    /// Insert a channel. Zero ID requests allocation.
    AddChannel {
        /// Channel to insert.
        channel: crate::channels::DocumentChannel,
    },
    /// Delete a named plane.
    DeleteChannel {
        /// Channel identity.
        id: crate::channels::ChannelId,
    },
    /// Change a channel's name.
    RenameChannel {
        /// Channel identity.
        id: crate::channels::ChannelId,
        /// New display name.
        name: String,
    },
    /// Replace samples and metadata, retaining identity and order.
    EditChannel {
        /// Replacement channel with an existing identity.
        channel: crate::channels::DocumentChannel,
    },
    /// Changes the shared light direction and invalidates all styled layers.
    SetGlobalLight(crate::render::styles::GlobalLight),
    /// Inserts a validated transform at an exact filter-stack position.
    AddTransform {
        /// Smart-object layer (convert pixel layers first).
        id: LayerId,
        /// Insertion index, at most the current stack length.
        index: usize,
        /// Non-destructive operation in child coordinates.
        transform: transform::TransformOp,
    },
    /// Replaces only an existing transform, preserving its blend and enabled state.
    SetTransform {
        /// Smart-object layer.
        id: LayerId,
        /// Index of an existing reserved transform node.
        index: usize,
        /// Replacement operation.
        transform: transform::TransformOp,
    },
    /// Replaces the non-destructive filter stack and shared child-space mask.
    SetSmartFilters {
        /// Smart-object layer.
        id: LayerId,
        /// Input-to-output evaluation order.
        filters: Vec<crate::document::SmartFilter>,
        /// Shared filter mask, applied after the entire stack.
        mask: Option<Mask>,
    },
    /// Inserts `layer` (ids assigned when zero) at `index` of `parent`.
    AddLayer {
        /// Group to insert into (`None` = root).
        parent: Option<LayerId>,
        /// Position, 0 = bottom; clamped.
        index: usize,
        /// The layer.
        layer: Layer,
    },
    /// Removes a layer and its descendants.
    RemoveLayer {
        /// Layer.
        id: LayerId,
    },
    /// Moves a layer to `index` of `parent`.
    MoveLayer {
        /// Layer.
        id: LayerId,
        /// New parent.
        parent: Option<LayerId>,
        /// New index (after removal), clamped.
        index: usize,
    },
    /// Duplicates a layer (deep, fresh ids) directly above it. Tiles are
    /// shared copy-on-write.
    DuplicateLayer {
        /// Layer.
        id: LayerId,
    },
    /// Replaces the common properties.
    SetProps {
        /// Layer.
        id: LayerId,
        /// New properties.
        props: LayerProps,
    },
    /// Replaces (or removes) the layer mask.
    SetMask {
        /// Layer.
        id: LayerId,
        /// New mask.
        mask: Option<Mask>,
    },
    /// Replaces an adjustment layer's parameters.
    SetAdjustment {
        /// Layer.
        id: LayerId,
        /// Parameters.
        adjustment: Adjustment,
    },
    /// Replaces a fill layer's content.
    SetFill {
        /// Layer.
        id: LayerId,
        /// Content.
        fill: Fill,
    },
    /// Switches a group between pass-through and isolated.
    SetGroupMode {
        /// Group.
        id: LayerId,
        /// Mode.
        mode: GroupMode,
    },
    /// Replaces a smart object's transform.
    SetSmartTransform {
        /// Smart object layer.
        id: LayerId,
        /// Child → parent map.
        transform: Affine,
    },
    /// Applies an op inside a smart object's child document.
    EditSmartObject {
        /// Smart object layer.
        id: LayerId,
        /// The nested op.
        op: Box<DocOp>,
    },
    /// Installs new tiles (a brush stroke's tile deltas).
    PaintTiles {
        /// Layer.
        id: LayerId,
        /// Content or mask.
        target: PaintTarget,
        /// Replaced tiles.
        tiles: Vec<TileDelta>,
        /// Level-0 pixels that may differ; everything else in the tiles must
        /// be unchanged (drives dirty-rect compositing).
        dirty: Rect,
    },
    /// Replaces the selection.
    SetSelection {
        /// New selection (`None` = deselect).
        selection: Option<Raster>,
    },
    /// Several ops as one history state.
    Batch(Vec<DocOp>),
}

impl DocOp {
    /// Short label for the history panel.
    pub fn label(&self) -> String {
        match self {
            DocOp::AutoAlignLayers { .. } => "Auto-Align Layers".into(),
            DocOp::AutoBlendLayers { .. } => "Auto-Blend Layers".into(),
            DocOp::Photomerge { .. } => "Photomerge".into(),
            DocOp::AddChannel { .. } => "Add Channel".into(),
            DocOp::DeleteChannel { .. } => "Delete Channel".into(),
            DocOp::RenameChannel { .. } => "Rename Channel".into(),
            DocOp::EditChannel { .. } => "Edit Channel".into(),
            DocOp::SetGlobalLight(_) => "Global Light".into(),
            DocOp::SetSmartFilters { .. } => "Smart Filters".into(),
            DocOp::AddTransform { .. } => "Add Transform".into(),
            DocOp::SetTransform { .. } => "Set Transform".into(),
            DocOp::AddLayer { layer, .. } => format!("New Layer {}", layer.props.name),
            DocOp::RemoveLayer { .. } => "Delete Layer".into(),
            DocOp::MoveLayer { .. } => "Move Layer".into(),
            DocOp::DuplicateLayer { .. } => "Duplicate Layer".into(),
            DocOp::SetProps { .. } => "Layer Properties".into(),
            DocOp::SetMask { .. } => "Layer Mask".into(),
            DocOp::SetAdjustment { .. } => "Modify Adjustment".into(),
            DocOp::SetFill { .. } => "Modify Fill".into(),
            DocOp::SetGroupMode { .. } => "Group Mode".into(),
            DocOp::SetSmartTransform { .. } => "Transform Smart Object".into(),
            DocOp::EditSmartObject { op, .. } => format!("Smart Object: {}", op.label()),
            DocOp::PaintTiles { .. } => "Paint".into(),
            DocOp::SetSelection { .. } => "Selection".into(),
            DocOp::Batch(v) => v
                .first()
                .map(|o| o.label())
                .unwrap_or_else(|| "Batch".into()),
        }
    }
}

/// What [`Document::apply`] did.
#[derive(Debug, Clone)]
pub struct Applied {
    /// Revision allocated to the op.
    pub rev: u64,
    /// Level-0 canvas region whose composite may have changed.
    pub damage: Rect,
    /// Ids created (added or duplicated layers, roots of subtrees first).
    pub created: Vec<LayerId>,
    /// The new history node.
    pub node: u64,
}

fn layer_damage(state: &DocState, layer: &Layer) -> Rect {
    let full = Rect::of_extent(state.canvas);
    layer.affected_bounds().map_or(full, |r| r.intersect(&full))
}

fn not_found(id: LayerId) -> EngineError {
    EngineError::not_found("layer", id.0)
}

/// Applies `op` to `s` with revision `rev`, returning the damage.
fn apply_op(
    s: &mut DocState,
    op: DocOp,
    rev: u64,
    created: &mut Vec<LayerId>,
) -> EngineResult<Rect> {
    let full = Rect::of_extent(s.canvas);
    let styled_before = s.has_layer_styles();
    let damage: EngineResult<Rect> = match op {
        DocOp::AutoAlignLayers { ids, options } => {
            align_document_layers(s, &ids, &options, rev)?;
            Ok(full.union(&Rect::of_extent(s.canvas)))
        }
        DocOp::AutoBlendLayers { ids, options, fill } => {
            blend_document_layers(s, &ids, &options, fill, rev, created)?;
            Ok(full)
        }
        DocOp::Photomerge {
            images,
            align,
            blend,
            fill,
        } => {
            if images.is_empty() || images.len() > 128 {
                return Err(EngineError::invalid("photomerge", "expected 1..128 images"));
            }
            let mut ids = Vec::new();
            for (name, image) in images {
                image.validate().map_err(merge_error)?;
                let extent = engine_api::tile::Extent::new(image.width as u32, image.height as u32);
                let mut raster = Raster::new(extent, 4, crate::Depth::F32, 0.);
                raster.edit_region(Rect::of_extent(extent), rev, |x, y, p| {
                    let rgb = image.pixels[y as usize * image.width + x as usize];
                    *p = [rgb[0], rgb[1], rgb[2], 1.];
                })?;
                let mut layer = Layer::new(name, LayerKind::Pixel(raster));
                s.assign_ids(&mut layer, false);
                stamp_new(&mut layer, rev);
                ids.push(layer.id);
                created.push(layer.id);
                s.root.push(Arc::new(layer));
            }
            align_document_layers(s, &ids, &align, rev)?;
            blend_document_layers(s, &ids, &blend, fill, rev, created)?;
            Ok(full.union(&Rect::of_extent(s.canvas)))
        }
        DocOp::AddChannel { mut channel } => {
            channel.validate(s)?;
            let largest = s.channels.iter().map(|c| c.id.0).max().unwrap_or(0);
            let next = s
                .next_channel_id
                .max(
                    largest
                        .checked_add(1)
                        .ok_or_else(|| EngineError::invalid("channel", "id exhausted"))?,
                )
                .max(1);
            if channel.id.0 == 0 {
                channel.id = crate::channels::ChannelId(next);
            }
            if s.channels.iter().any(|c| c.id == channel.id) {
                return Err(EngineError::invalid("channel", "duplicate id"));
            }
            s.next_channel_id = next.max(
                channel
                    .id
                    .0
                    .checked_add(1)
                    .ok_or_else(|| EngineError::invalid("channel", "id exhausted"))?,
            );
            s.channels.push(channel);
            Ok(Rect::default())
        }
        DocOp::DeleteChannel { id } => {
            let i = s
                .channels
                .iter()
                .position(|c| c.id == id)
                .ok_or_else(|| EngineError::not_found("channel", id.0))?;
            s.channels.remove(i);
            Ok(Rect::default())
        }
        DocOp::RenameChannel { id, name } => {
            let c = s
                .channels
                .iter_mut()
                .find(|c| c.id == id)
                .ok_or_else(|| EngineError::not_found("channel", id.0))?;
            c.name = name;
            Ok(Rect::default())
        }
        DocOp::EditChannel { channel } => {
            channel.validate(s)?;
            let c = s
                .channels
                .iter_mut()
                .find(|c| c.id == channel.id)
                .ok_or_else(|| EngineError::not_found("channel", channel.id.0))?;
            *c = channel;
            Ok(Rect::default())
        }
        DocOp::AddLayer {
            parent,
            index,
            mut layer,
        } => {
            s.assign_ids(&mut layer, false);
            if s.layer_ids().contains(&layer.id) {
                return Err(EngineError::invalid("layer", "duplicate id"));
            }
            stamp_new(&mut layer, rev);
            created.push(layer.id);
            let damage = layer_damage(s, &layer);
            let v = s.children_mut(parent)?;
            let i = index.min(v.len());
            v.insert(i, Arc::new(layer));
            if parent.is_none() {
                s.root_rev = rev;
            } else if let Some(p) = parent {
                s.layer_mut(p, |g| g.content_rev = rev);
            }
            Ok(damage)
        }
        DocOp::RemoveLayer { id } => {
            let (parent, i) = s.locate(id).ok_or_else(|| not_found(id))?;
            let damage = layer_damage(s, s.find(id).ok_or_else(|| not_found(id))?);
            s.children_mut(parent)?.remove(i);
            bump_parent(s, parent, rev);
            Ok(damage)
        }
        DocOp::MoveLayer { id, parent, index } => {
            let (old_parent, i) = s.locate(id).ok_or_else(|| not_found(id))?;
            if let Some(p) = parent {
                let l = s.find(id).ok_or_else(|| not_found(id))?;
                if p == id
                    || l.children()
                        .is_some_and(|c| crate::document::contains_id(c, p))
                {
                    return Err(EngineError::invalid(
                        "parent",
                        "cannot move a group into itself",
                    ));
                }
            }
            let damage = layer_damage(s, s.find(id).ok_or_else(|| not_found(id))?);
            let layer = s.children_mut(old_parent)?.remove(i);
            bump_parent(s, old_parent, rev);
            let v = s.children_mut(parent)?;
            let j = index.min(v.len());
            v.insert(j, layer);
            bump_parent(s, parent, rev);
            Ok(damage)
        }
        DocOp::DuplicateLayer { id } => {
            let (parent, i) = s.locate(id).ok_or_else(|| not_found(id))?;
            let mut dup = s.find(id).ok_or_else(|| not_found(id))?.clone();
            s.assign_ids(&mut dup, true);
            dup.props.name = format!("{} copy", dup.props.name);
            dup.props.background = false;
            // Props/content revs are kept: tiles are shared and identical.
            // The parent's structure revision changes, which is what
            // invalidates the composite.
            created.push(dup.id);
            let damage = layer_damage(s, &dup);
            s.children_mut(parent)?.insert(i + 1, Arc::new(dup));
            bump_parent(s, parent, rev);
            Ok(damage)
        }
        DocOp::SetProps { id, props } => {
            let damage = layer_damage(s, s.find(id).ok_or_else(|| not_found(id))?);
            s.layer_mut(id, |l| {
                l.props = props;
                l.props_rev = rev;
            })
            .ok_or_else(|| not_found(id))?;
            Ok(damage)
        }
        DocOp::SetMask { id, mask } => {
            let damage = layer_damage(s, s.find(id).ok_or_else(|| not_found(id))?);
            if let Some(m) = &mask
                && (m.raster.extent() != s.canvas
                    || m.raster.channels() != 1
                    || m.raster.depth() != s.depth)
            {
                return Err(EngineError::invalid(
                    "mask",
                    "must be single-channel, canvas-sized, document depth",
                ));
            }
            s.layer_mut(id, |l| {
                l.mask = mask.map(|mut m| {
                    restamp(&mut m.raster, rev);
                    m
                });
                l.content_rev = rev;
            })
            .ok_or_else(|| not_found(id))?;
            Ok(damage)
        }
        DocOp::SetAdjustment { id, adjustment } => {
            s.layer_mut(id, |l| match &mut l.kind {
                LayerKind::Adjustment(a) => {
                    *a = adjustment;
                    l.content_rev = rev;
                    Ok(())
                }
                _ => Err(EngineError::invalid("layer", "not an adjustment layer")),
            })
            .ok_or_else(|| not_found(id))??;
            Ok(full)
        }
        DocOp::SetFill { id, fill } => {
            s.layer_mut(id, |l| match &mut l.kind {
                LayerKind::Fill(f) => {
                    *f = fill;
                    l.content_rev = rev;
                    Ok(())
                }
                _ => Err(EngineError::invalid("layer", "not a fill layer")),
            })
            .ok_or_else(|| not_found(id))??;
            Ok(full)
        }
        DocOp::SetGroupMode { id, mode } => {
            let damage = layer_damage(s, s.find(id).ok_or_else(|| not_found(id))?);
            s.layer_mut(id, |l| match &mut l.kind {
                LayerKind::Group { mode: m, .. } => {
                    *m = mode;
                    l.content_rev = rev;
                    Ok(())
                }
                _ => Err(EngineError::invalid("layer", "not a group")),
            })
            .ok_or_else(|| not_found(id))??;
            Ok(damage)
        }
        DocOp::SetGlobalLight(light) => {
            light.validate()?;
            s.global_light = light;
            s.root_rev = rev;
            Ok(full)
        }
        DocOp::AddTransform {
            id,
            index,
            transform,
        } => {
            let filter = crate::document::SmartFilter::transform(transform)?;
            s.layer_mut(id, |l| {
                if l.props.locks.all || l.props.locks.position {
                    return Err(EngineError::invalid("layer", "transform is locked"));
                }
                match &mut l.kind {
                    LayerKind::SmartObject(so) if index <= so.filters.len() => {
                        so.filters.insert(index, filter);
                        l.content_rev = rev;
                        Ok(())
                    }
                    _ => Err(EngineError::invalid(
                        "transform",
                        "smart object and valid insertion index required",
                    )),
                }
            })
            .ok_or_else(|| not_found(id))??;
            Ok(full)
        }
        DocOp::SetTransform {
            id,
            index,
            transform,
        } => {
            let filter = crate::document::SmartFilter::transform(transform)?;
            s.layer_mut(id, |l| {
                if l.props.locks.all || l.props.locks.position {
                    return Err(EngineError::invalid("layer", "transform is locked"));
                }
                let LayerKind::SmartObject(so) = &mut l.kind else {
                    return Err(EngineError::invalid("layer", "not a smart object"));
                };
                let old = so
                    .filters
                    .get_mut(index)
                    .filter(|f| f.name == "transform")
                    .ok_or_else(|| {
                        EngineError::invalid("transform", "index is not a transform stage")
                    })?;
                old.params = filter.params;
                l.content_rev = rev;
                Ok(())
            })
            .ok_or_else(|| not_found(id))??;
            Ok(full)
        }
        DocOp::SetSmartFilters {
            id,
            filters,
            mut mask,
        } => {
            for filter in &filters {
                filter.transform_op()?;
            }
            if let Some(mask) = &mut mask {
                restamp(&mut mask.raster, rev);
            }
            s.layer_mut(id, |l| match &mut l.kind {
                LayerKind::SmartObject(so) => {
                    // Replacing the whole stack must not bypass Add/SetTransform
                    // locks. Include stack indices and enabled/blend options:
                    // moving or disabling a transform also changes geometry.
                    let changes_transforms = so
                        .filters
                        .iter()
                        .enumerate()
                        .filter(|(_, f)| f.name == "transform")
                        .ne(filters
                            .iter()
                            .enumerate()
                            .filter(|(_, f)| f.name == "transform"));
                    if l.props.locks.all || (l.props.locks.position && changes_transforms) {
                        return Err(EngineError::invalid("layer", "transform is locked"));
                    }
                    so.filters = filters;
                    so.filter_mask = mask;
                    l.content_rev = rev;
                    Ok(())
                }
                _ => Err(EngineError::invalid("layer", "not a smart object")),
            })
            .ok_or_else(|| not_found(id))??;
            Ok(full)
        }
        DocOp::SetSmartTransform { id, transform } => {
            if transform.inverse().is_none() {
                return Err(EngineError::invalid("transform", "singular"));
            }
            s.layer_mut(id, |l| match &mut l.kind {
                LayerKind::SmartObject(so) => {
                    if l.props.locks.all || l.props.locks.position {
                        return Err(EngineError::invalid("layer", "transform is locked"));
                    }
                    so.transform = transform;
                    l.content_rev = rev;
                    Ok(())
                }
                _ => Err(EngineError::invalid("layer", "not a smart object")),
            })
            .ok_or_else(|| not_found(id))??;
            Ok(full)
        }
        DocOp::EditSmartObject { id, op } => {
            let mut nested = Vec::new();
            s.layer_mut(id, |l| match &mut l.kind {
                LayerKind::SmartObject(so) => {
                    let mut child = (*so.state).clone();
                    apply_op(&mut child, *op, rev, &mut nested)?;
                    child.rev = rev;
                    so.state = Arc::new(child);
                    l.content_rev = rev;
                    Ok(())
                }
                _ => Err(EngineError::invalid("layer", "not a smart object")),
            })
            .ok_or_else(|| not_found(id))??;
            Ok(full)
        }
        DocOp::PaintTiles {
            id,
            target,
            tiles,
            dirty,
        } => {
            let layer = s.find(id).ok_or_else(|| not_found(id))?;
            if layer.props.locks.pixels || layer.props.locks.all {
                return Err(EngineError::invalid("layer", "pixels are locked"));
            }
            let mut damage = Rect::default();
            s.layer_mut(id, |l| -> EngineResult<()> {
                let r = match target {
                    PaintTarget::Content => l.raster_mut(),
                    PaintTarget::Mask => l.mask.as_mut().map(|m| &mut m.raster),
                }
                .ok_or_else(|| EngineError::invalid("layer", "no raster for paint target"))?;
                for d in tiles {
                    let (x0, y0) = (i64::from(d.tx * TILE_SIZE), i64::from(d.ty * TILE_SIZE));
                    let lay = r.layout(d.tx, d.ty);
                    let tr = Rect::new(
                        x0,
                        y0,
                        x0 + i64::from(lay.extent.width),
                        y0 + i64::from(lay.extent.height),
                    );
                    damage = damage.union(&tr.intersect(&dirty));
                    let tile = d.tile.map(|t| retag(t, d.tx, d.ty)).transpose()?;
                    r.set_slot(d.tx, d.ty, tile, rev)?;
                }
                Ok(())
            })
            .ok_or_else(|| not_found(id))??;
            Ok(damage)
        }
        DocOp::SetSelection { selection } => {
            if let Some(sel) = &selection
                && (sel.extent() != s.canvas || sel.channels() != 1)
            {
                return Err(EngineError::invalid(
                    "selection",
                    "must be single-channel, canvas-sized",
                ));
            }
            s.selection = selection.map(Arc::new);
            Ok(Rect::default())
        }
        DocOp::Batch(ops) => {
            let mut d = Rect::default();
            for op in ops {
                d = d.union(&apply_op(s, op, rev, created)?);
            }
            Ok(d)
        }
    };
    let damage = damage?;
    Ok(if styled_before || s.has_layer_styles() {
        full
    } else {
        damage
    })
}

fn merge_error(message: String) -> EngineError {
    EngineError::invalid("layer merge", message)
}

fn merge_indices(s: &DocState, ids: &[LayerId]) -> EngineResult<Vec<usize>> {
    if ids.is_empty() || ids.len() > 128 {
        return Err(merge_error("expected 1..128 root layers".into()));
    }
    let mut indices = Vec::new();
    for id in ids {
        let i = s
            .root
            .iter()
            .position(|l| l.id == *id)
            .ok_or_else(|| not_found(*id))?;
        if indices.contains(&i) {
            return Err(merge_error("duplicate layer selection".into()));
        }
        let l = &s.root[i];
        if l.props.locks.all || l.props.locks.pixels || l.props.locks.position {
            return Err(merge_error("selected layer is locked".into()));
        }
        if l.props.clipped
            || !matches!(
                l.kind,
                LayerKind::Pixel(_) | LayerKind::SmartObject(_) | LayerKind::Text(_)
            )
        {
            return Err(merge_error(
                "merge requires independent root image layers".into(),
            ));
        }
        indices.push(i);
    }
    Ok(indices)
}

fn linear_image(r: &Raster) -> merge::LinearImage {
    let e = r.extent();
    merge::LinearImage {
        width: e.width as usize,
        height: e.height as usize,
        pixels: (0..e.height)
            .flat_map(|y| {
                (0..e.width).map(move |x| {
                    let p = r.pixel(x, y);
                    [p[0], p[1], p[2]]
                })
            })
            .collect(),
        color_matrix: [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]],
        as_shot_neutral: [1.; 3],
    }
}

fn shift_plane(
    r: &Raster,
    extent: engine_api::tile::Extent,
    dx: u32,
    dy: u32,
    rev: u64,
) -> EngineResult<Raster> {
    let mut out = Raster::new(extent, r.channels(), r.depth(), 0.);
    out.edit_region(Rect::of_extent(extent), rev, |x, y, p| {
        if x >= dx && y >= dy && x - dx < r.extent().width && y - dy < r.extent().height {
            *p = r.pixel(x - dx, y - dy);
        }
    })?;
    Ok(out)
}

// Retain the original pixels/masks inside a child; layer-level appearance stays
// on the wrapper so opacity and blend modes are not applied twice.
fn source_child(layer: &Layer, extent: engine_api::tile::Extent, depth: crate::Depth) -> DocState {
    let mut source = layer.clone();
    source.id = LayerId(1);
    source.props = LayerProps {
        name: "Original source".into(),
        ..Default::default()
    };
    // Pixel tiles at a source's right/bottom edge have native-size layouts.
    // Rendering those directly under a larger canvas violates the tile-copy
    // contract. Nest a native-size document instead of resizing/retagging tiles.
    if let Some(raster) = source.raster()
        && raster.extent() != extent
    {
        let mut native = DocState::new(raster.extent(), depth);
        native.root.push(Arc::new(source));
        native.next_id = 2;
        source = Layer::new(
            "Original source",
            LayerKind::SmartObject(crate::document::SmartObject::new(native, Affine::IDENTITY)),
        );
        source.id = LayerId(1);
    }
    let mut child = DocState::new(extent, depth);
    child.root.push(Arc::new(source));
    child.next_id = 2;
    child
}

fn align_document_layers(
    s: &mut DocState,
    ids: &[LayerId],
    options: &merge::layers::AlignOptions,
    rev: u64,
) -> EngineResult<()> {
    let indices = merge_indices(s, ids)?;
    let mut images = Vec::new();
    for &i in &indices {
        let LayerKind::Pixel(r) = &s.root[i].kind else {
            return Err(merge_error(
                "alignment currently requires pixel layers".into(),
            ));
        };
        if r.channels() != 4 {
            return Err(merge_error("alignment requires RGBA rasters".into()));
        }
        images.push(linear_image(r));
    }
    let aligned = merge::layers::align_layers(&images, options).map_err(merge_error)?;
    let dx = (-aligned.origin[0]).max(0.) as u32;
    let dy = (-aligned.origin[1]).max(0.) as u32;
    let canvas = engine_api::tile::Extent::new(
        (aligned.width as u32).max(
            s.canvas
                .width
                .checked_add(dx)
                .ok_or_else(|| merge_error("canvas overflow".into()))?,
        ),
        (aligned.height as u32).max(
            s.canvas
                .height
                .checked_add(dy)
                .ok_or_else(|| merge_error("canvas overflow".into()))?,
        ),
    );
    if u64::from(canvas.width) * u64::from(canvas.height) > 64 * 1024 * 1024 {
        return Err(merge_error("document union exceeds 64 MP".into()));
    }
    for (i, arc) in s.root.iter_mut().enumerate() {
        let old = arc.as_ref();
        let selected = indices.iter().position(|j| *j == i);
        if selected.is_none() && dx == 0 && dy == 0 {
            continue;
        }
        if selected.is_none()
            && (old.props.locks.all
                || old.props.locks.position
                || old.is_group()
                || old.props.clipped)
        {
            return Err(merge_error(
                "cannot shift locked/grouped/clipped unselected layers".into(),
            ));
        }
        let (child_extent, transform) = if let Some(k) = selected {
            (
                engine_api::tile::Extent::new(
                    canvas.width.max(images[k].width as u32),
                    canvas.height.max(images[k].height as u32),
                ),
                Some(aligned.transforms[k].clone()),
            )
        } else {
            (s.canvas, None)
        };
        let mut child = source_child(old, child_extent, s.depth);
        if let Some(k) = selected
            && let Some(gains) = aligned.source_gains.get(k)
        {
            // Linear, unclamped gain before geometry; keep source and masks intact.
            child.depth = crate::Depth::F32;
            let mut raster = Raster::new(child_extent, 4, crate::Depth::F32, 0.);
            raster.edit_region(Rect::of_extent(child_extent), rev, |x, y, p| {
                let gain = if (x as usize) < images[k].width && (y as usize) < images[k].height {
                    gains[y as usize * images[k].width + x as usize]
                } else {
                    1.
                };
                *p = [gain, gain, gain, 1.];
            })?;
            let mut correction = Layer::new("Vignette removal", LayerKind::Pixel(raster));
            correction.id = LayerId(2);
            correction.props.clipped = true;
            correction.props.blend_mode = crate::BlendMode::Multiply;
            child.root.push(Arc::new(correction));
            child.next_id = 3;
        }
        let mut so = crate::document::SmartObject::new(child, Affine::IDENTITY);
        if let Some(op) = transform {
            so.filters
                .push(crate::document::SmartFilter::transform(op)?);
        } else {
            so.transform = Affine {
                m: [1., 0., dx as f64, 0., 1., dy as f64],
            };
        }
        let layer = Arc::make_mut(arc);
        layer.kind = LayerKind::SmartObject(so);
        layer.mask = None;
        layer.vector_mask = None;
        layer.content_rev = rev;
        layer.props_rev = rev;
    }
    if let Some(selection) = &s.selection {
        s.selection = Some(Arc::new(shift_plane(selection, canvas, dx, dy, rev)?));
    }
    for channel in &mut s.channels {
        channel.raster = shift_plane(&channel.raster, canvas, dx, dy, rev)?;
    }
    s.canvas = canvas;
    s.root_rev = rev;
    Ok(())
}

fn render_merge_source(s: &DocState, layer: &Layer) -> EngineResult<Raster> {
    let child = source_child(layer, s.canvas, crate::Depth::F32);
    let (_, pixels) =
        crate::Compositor::new(64 << 20).render_level_rgba(&Document::new(child), 0)?;
    let mut out = Raster::new(s.canvas, 4, crate::Depth::F32, 0.);
    out.edit_region(Rect::of_extent(s.canvas), 1, |x, y, p| {
        let i = ((y * s.canvas.width + x) * 4) as usize;
        p.copy_from_slice(&pixels[i..i + 4]);
    })?;
    Ok(out)
}

fn blend_document_layers(
    s: &mut DocState,
    ids: &[LayerId],
    options: &merge::layers::BlendOptions,
    fill: Option<ContentAwareFill>,
    rev: u64,
    created: &mut Vec<LayerId>,
) -> EngineResult<()> {
    let indices = merge_indices(s, ids)?;
    if options.fill_transparent && fill.is_none() {
        return Err(merge_error(
            "content-aware fill requires a filters::caf adapter".into(),
        ));
    }
    let mut images = Vec::new();
    let mut coverage = Vec::new();
    for &i in &indices {
        let r = render_merge_source(s, &s.root[i])?;
        images.push(linear_image(&r));
        coverage.push(
            (0..s.canvas.height)
                .flat_map(|y| {
                    let r = &r;
                    (0..s.canvas.width).map(move |x| r.pixel(x, y)[3] > 1e-6)
                })
                .collect(),
        );
    }
    let blended = merge::layers::blend_layers(&images, &coverage, options).map_err(merge_error)?;
    for (k, &i) in indices.iter().enumerate() {
        let mut child = source_child(&s.root[i], s.canvas, crate::Depth::F32);
        if blended.corrections[k].iter().flatten().any(|v| *v != 0.) {
            let mut delta = Raster::new(s.canvas, 4, crate::Depth::F32, 0.);
            delta.edit_region(Rect::of_extent(s.canvas), rev, |x, y, p| {
                let d = blended.corrections[k][y as usize * s.canvas.width as usize + x as usize];
                *p = [d[0], d[1], d[2], 1.];
            })?;
            let mut correction = Layer::new("Seamless tones and colors", LayerKind::Pixel(delta));
            correction.id = LayerId(2);
            correction.props.clipped = true;
            correction.props.blend_mode = crate::BlendMode::LinearDodge;
            child.root.push(Arc::new(correction));
            child.next_id = 3;
        }
        let mut mask = Mask::hide_all(s.canvas, s.depth);
        mask.raster
            .edit_region(Rect::of_extent(s.canvas), rev, |x, y, p| {
                p[0] = blended.masks[k][y as usize * s.canvas.width as usize + x as usize];
            })?;
        let layer = Arc::make_mut(&mut s.root[i]);
        layer.kind =
            LayerKind::SmartObject(crate::document::SmartObject::new(child, Affine::IDENTITY));
        layer.mask = Some(mask);
        layer.vector_mask = None;
        layer.content_rev = rev;
        layer.props_rev = rev;
    }
    if blended.fill_mask.iter().any(|v| *v) {
        let mut raster = Raster::new(s.canvas, 4, crate::Depth::F32, 0.);
        raster.edit_region(Rect::of_extent(s.canvas), rev, |x, y, p| {
            let j = y as usize * s.canvas.width as usize + x as usize;
            let rgb = blended.image.pixels[j];
            *p = [rgb[0], rgb[1], rgb[2], f32::from(blended.coverage[j])];
        })?;
        let holes: Vec<_> = blended.fill_mask.iter().map(|b| f32::from(*b)).collect();
        let filled = fill.ok_or_else(|| merge_error("missing CAF adapter".into()))?(
            &raster,
            &holes,
            options.seed,
        )?;
        if filled.extent() != s.canvas || filled.channels() != 4 {
            return Err(merge_error(
                "CAF returned invalid dimensions/channels".into(),
            ));
        }
        raster.edit_region(Rect::of_extent(s.canvas), rev, |x, y, p| {
            let j = y as usize * s.canvas.width as usize + x as usize;
            *p = if holes[j] > 0. {
                filled.pixel(x, y)
            } else {
                [0.; 4]
            };
        })?;
        let mut layer = Layer::new("Content-aware fill", LayerKind::Pixel(raster));
        s.assign_ids(&mut layer, false);
        stamp_new(&mut layer, rev);
        created.push(layer.id);
        s.root.push(Arc::new(layer));
    }
    s.root_rev = rev;
    Ok(())
}

fn retag(t: Tile, tx: u32, ty: u32) -> EngineResult<Tile> {
    let coord = engine_api::tile::TileCoord::new(0, tx, ty);
    if t.coord() == coord {
        return Ok(t);
    }
    // Re-wrap with the right coordinate; shares nothing but is rare.
    let layout = t.layout();
    match t.format() {
        engine_api::tile::TileFormat::U8 => {
            Tile::from_samples(coord, layout, t.samples::<u8>()?.to_vec())
        }
        engine_api::tile::TileFormat::U16 => {
            Tile::from_samples(coord, layout, t.samples::<u16>()?.to_vec())
        }
        engine_api::tile::TileFormat::F32Planar => {
            Tile::from_samples(coord, layout, t.samples::<f32>()?.to_vec())
        }
        engine_api::tile::TileFormat::F16Planar => {
            Tile::from_samples(coord, layout, t.samples::<half::f16>()?.to_vec())
        }
    }
}

fn bump_parent(s: &mut DocState, parent: Option<LayerId>, rev: u64) {
    match parent {
        None => s.root_rev = rev,
        Some(p) => {
            s.layer_mut(p, |g| g.content_rev = rev);
        }
    }
}

/// Gives a newly added layer (and its subtree) revision `rev` everywhere,
/// so no stale cache entry can match it.
fn stamp_new(layer: &mut Layer, rev: u64) {
    layer.props_rev = rev;
    layer.content_rev = rev;
    if let Some(m) = &mut layer.mask {
        restamp(&mut m.raster, rev);
    }
    match &mut layer.kind {
        LayerKind::Pixel(r) => restamp(r, rev),
        LayerKind::Text(t) => restamp(&mut t.proxy, rev),
        LayerKind::Group { children, .. } => {
            for c in children.iter_mut() {
                stamp_new(Arc::make_mut(c), rev);
            }
        }
        LayerKind::SmartObject(so) => {
            let mut st = (*so.state).clone();
            st.rev = st.rev.max(rev);
            so.state = Arc::new(st);
            so.key = next_doc_key();
        }
        _ => {}
    }
}

fn restamp(r: &mut Raster, rev: u64) {
    let slots: Vec<_> = r.slots().map(|(k, s)| (k, s.tile.clone())).collect();
    for ((tx, ty), t) in slots {
        let _ = r.set_slot(tx, ty, t, rev);
    }
}

/// One history state.
#[derive(Debug, Clone)]
pub struct HistoryNode {
    /// Node id (creation order).
    pub id: u64,
    /// Parent node (`None` for the root or after pruning).
    pub parent: Option<u64>,
    /// Label of the op that produced it.
    pub label: String,
    /// Revision of that op.
    pub rev: u64,
    /// The snapshot.
    pub state: Arc<DocState>,
}

/// Non-linear history: a tree of structurally shared snapshots.
#[derive(Debug, Clone)]
pub struct History {
    nodes: BTreeMap<u64, HistoryNode>,
    current: u64,
    next: u64,
    snapshots: BTreeMap<String, u64>,
    /// Maximum retained nodes (the current node and named snapshots are
    /// never pruned; the oldest others go first).
    pub max_states: usize,
}

/// A document: current state, history and the damage log used for
/// dirty-rect compositing. Cloning yields an independent document with a
/// new cache namespace.
#[derive(Debug)]
pub struct Document {
    pub(crate) psd_source: Option<Arc<crate::psd::ImportedPsd>>,
    key: u64,
    history: History,
    epoch: u64,
    damage: VecDeque<(u64, Rect)>,
    damage_floor: u64,
}

const DAMAGE_LOG: usize = 4096;

impl Clone for Document {
    fn clone(&self) -> Self {
        Self {
            psd_source: self.psd_source.clone(),
            key: next_doc_key(),
            history: self.history.clone(),
            epoch: self.epoch,
            damage: self.damage.clone(),
            damage_floor: self.damage_floor,
        }
    }
}

impl Document {
    /// Rasterize each root image layer's transforms while retaining its editable
    /// mask and layer properties, for layered PSD export. Unlike flattened
    /// export this preserves seam/focus masks. Groups and adjustments must be
    /// exported natively or flattened explicitly, not silently reinterpreted.
    pub fn rasterized_layers_for_export(&self) -> EngineResult<Self> {
        let mut state = (**self.state()).clone();
        for arc in &mut state.root {
            if !matches!(
                arc.kind,
                LayerKind::Pixel(_) | LayerKind::SmartObject(_) | LayerKind::Text(_)
            ) {
                return Err(merge_error(
                    "layer rasterization supports root image layers only".into(),
                ));
            }
            let mut source = arc.as_ref().clone();
            source.mask = None;
            source.vector_mask = None;
            let raster = render_merge_source(self.state(), &source)?;
            let layer = Arc::make_mut(arc);
            layer.kind = LayerKind::Pixel(raster);
            layer.content_rev = next_rev();
        }
        state.root_rev = next_rev();
        state.rev = state.root_rev;
        Ok(Self::new(state))
    }

    /// Explicit, lossy export proxy for formats without native smart filters.
    /// Renders the complete document (including masks/blends/transforms) using
    /// the caller's evaluator. The original editable document is not changed.
    pub fn rasterized_for_export(
        &self,
        renderer: &mut crate::Compositor,
    ) -> EngineResult<(Self, Vec<String>)> {
        let (extent, pixels) = renderer.render_level_rgba(self, 0)?;
        let mut raster = Raster::new(extent, 4, crate::raster::Depth::F32, 0.0);
        raster.edit_region(Rect::of_extent(extent), 1, |x, y, p| {
            let i = ((y * extent.width + x) * 4) as usize;
            p.copy_from_slice(&pixels[i..i + 4]);
        })?;
        let mut state = DocState::new(extent, crate::raster::Depth::F32);
        state.ppi = self.state().ppi;
        state.profile = self.state().profile.clone();
        let mut layer = Layer::new("Rasterized export", LayerKind::Pixel(raster));
        state.assign_ids(&mut layer, false);
        state.root.push(Arc::new(layer));
        Ok((Self::new(state), vec![
            "Document rasterized for PSD/export; smart filters and layers remain editable only in the native .tessera-doc original.".into(),
        ]))
    }

    /// Wraps an initial state as history node 0.
    pub fn new(mut state: DocState) -> Self {
        if state.rev == 0 {
            state.rev = next_rev();
        }
        let rev = state.rev;
        let root = HistoryNode {
            id: 0,
            parent: None,
            label: "Open".into(),
            rev,
            state: Arc::new(state),
        };
        Self {
            key: next_doc_key(),
            history: History {
                nodes: BTreeMap::from([(0, root)]),
                current: 0,
                next: 1,
                snapshots: BTreeMap::new(),
                max_states: 200,
            },
            epoch: 0,
            damage: VecDeque::new(),
            damage_floor: 0,
            psd_source: None,
        }
    }

    /// The current state.
    pub fn state(&self) -> &Arc<DocState> {
        &self.history.nodes[&self.history.current].state
    }

    /// Cache namespace of this document instance.
    pub fn key(&self) -> u64 {
        self.key
    }

    /// Increments on every history jump (undo/redo/checkout).
    pub fn epoch(&self) -> u64 {
        self.epoch
    }

    /// History access.
    pub fn history(&self) -> &History {
        &self.history
    }

    /// Mutable history settings.
    pub fn set_max_states(&mut self, n: usize) {
        self.history.max_states = n.max(2);
        self.prune();
    }

    /// Applies an op as a new history state (a branch if not at a leaf).
    pub fn apply(&mut self, op: DocOp) -> EngineResult<Applied> {
        let rev = next_rev();
        let label = op.label();
        let mut state = (**self.state()).clone();
        let mut created = Vec::new();
        let damage = apply_op(&mut state, op, rev, &mut created)?;
        state.rev = rev;
        let id = self.history.next;
        self.history.next += 1;
        self.history.nodes.insert(
            id,
            HistoryNode {
                id,
                parent: Some(self.history.current),
                label,
                rev,
                state: Arc::new(state),
            },
        );
        self.history.current = id;
        self.damage.push_back((rev, damage));
        while self.damage.len() > DAMAGE_LOG {
            if let Some((r, _)) = self.damage.pop_front() {
                self.damage_floor = self.damage_floor.max(r);
            }
        }
        self.prune();
        Ok(Applied {
            rev,
            damage,
            created,
            node: id,
        })
    }

    /// Jumps to any retained history node.
    pub fn checkout(&mut self, node: u64) -> EngineResult<()> {
        if !self.history.nodes.contains_key(&node) {
            return Err(EngineError::not_found("history state", node));
        }
        if node != self.history.current {
            self.history.current = node;
            self.jumped();
        }
        Ok(())
    }

    /// Steps to the parent state. Returns false at the root.
    pub fn undo(&mut self) -> bool {
        match self.history.nodes[&self.history.current].parent {
            Some(p) if self.history.nodes.contains_key(&p) => {
                self.history.current = p;
                self.jumped();
                true
            }
            _ => false,
        }
    }

    /// Steps to the most recently created child. Returns false at a leaf.
    pub fn redo(&mut self) -> bool {
        let cur = self.history.current;
        let child = self
            .history
            .nodes
            .values()
            .filter(|n| n.parent == Some(cur))
            .map(|n| n.id)
            .max();
        match child {
            Some(c) => {
                self.history.current = c;
                self.jumped();
                true
            }
            None => false,
        }
    }

    /// Names the current state.
    pub fn snapshot(&mut self, name: impl Into<String>) {
        self.history
            .snapshots
            .insert(name.into(), self.history.current);
    }

    /// Jumps to a named snapshot.
    pub fn restore_snapshot(&mut self, name: &str) -> EngineResult<()> {
        let node = *self
            .history
            .snapshots
            .get(name)
            .ok_or_else(|| EngineError::not_found("snapshot", name))?;
        self.checkout(node)
    }

    fn jumped(&mut self) {
        self.epoch += 1;
        self.damage.clear();
        self.damage_floor = 0;
    }

    fn prune(&mut self) {
        let h = &mut self.history;
        if h.nodes.len() <= h.max_states {
            return;
        }
        // Every node is a full (structurally shared) snapshot, so any node
        // but the current one and named snapshots can go; undo then lands
        // on the nearest retained ancestor.
        let mut keep: std::collections::BTreeSet<u64> = h.snapshots.values().copied().collect();
        keep.insert(h.current);
        let victims: Vec<u64> = h
            .nodes
            .keys()
            .copied()
            .filter(|k| !keep.contains(k))
            .take(h.nodes.len() - h.max_states)
            .collect();
        for v in victims {
            let parent = h.nodes.remove(&v).and_then(|n| n.parent);
            for n in h.nodes.values_mut() {
                if n.parent == Some(v) {
                    n.parent = parent;
                }
            }
        }
    }

    /// Union of logged damage with revision in `(after, upto]`, each entry
    /// clipped to `clip` (level-0 pixels) before the union, or `None` when
    /// the log cannot answer (history jump, truncation).
    pub(crate) fn damage_between(&self, after: u64, upto: u64, clip: Rect) -> Option<Rect> {
        if after < self.damage_floor || upto < after {
            return None;
        }
        let mut r = Rect::default();
        for (rev, d) in self.damage.iter().rev() {
            if *rev <= after {
                break;
            }
            if *rev <= upto {
                r = r.union(&d.intersect(&clip));
            }
        }
        Some(r)
    }

    /// Bytes of distinct tile buffers referenced by all retained history
    /// states (COW-shared tiles counted once).
    pub fn history_bytes(&self) -> usize {
        let mut seen = std::collections::HashSet::new();
        let mut bytes = 0;
        let mut count = |r: &Raster| {
            for (_, s) in r.slots() {
                if let Some(t) = &s.tile
                    && seen.insert(buffer_addr(t))
                {
                    bytes += t.byte_len();
                }
            }
        };
        for n in self.history.nodes.values() {
            visit_rasters(&n.state, &mut count);
        }
        bytes
    }
}

fn visit_rasters(s: &DocState, f: &mut impl FnMut(&Raster)) {
    for channel in &s.channels {
        f(&channel.raster);
    }
    if let Some(sel) = &s.selection {
        f(sel);
    }
    for l in &s.root {
        l.visit(&mut |l| {
            if let Some(r) = l.raster() {
                f(r);
            }
            if let Some(m) = &l.mask {
                f(&m.raster);
            }
            if let LayerKind::SmartObject(so) = &l.kind {
                visit_rasters(&so.state, f);
            }
        });
    }
}

impl History {
    /// The current node id.
    pub fn current(&self) -> u64 {
        self.current
    }

    /// All retained nodes in creation order.
    pub fn nodes(&self) -> impl Iterator<Item = &HistoryNode> {
        self.nodes.values()
    }

    /// Named snapshots.
    pub fn snapshots(&self) -> &BTreeMap<String, u64> {
        &self.snapshots
    }

    /// Number of retained nodes.
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// Never empty (the root is always retained while current).
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }
}

/// Builds a [`DocOp::PaintTiles`] that applies `f` to every pixel of
/// `rect` of a layer's content (honouring the transparency lock).
pub fn paint_op(
    state: &DocState,
    id: LayerId,
    target: PaintTarget,
    rect: Rect,
    f: impl FnMut(u32, u32, &mut [f32; 4]),
) -> EngineResult<DocOp> {
    let layer = state.find(id).ok_or_else(|| not_found(id))?;
    let raster = match target {
        PaintTarget::Content => layer.raster(),
        PaintTarget::Mask => layer.mask.as_ref().map(|m| &m.raster),
    }
    .ok_or_else(|| EngineError::invalid("layer", "no raster for paint target"))?;
    let lock_alpha = target == PaintTarget::Content && layer.props.locks.transparency;
    let mut f = f;
    let tiles = raster.render_region(rect, |x, y, p| {
        let a = p[3];
        f(x, y, p);
        if lock_alpha {
            p[3] = a;
        }
    })?;
    Ok(DocOp::PaintTiles {
        id,
        target,
        tiles: tiles
            .into_iter()
            .map(|(tx, ty, tile)| TileDelta {
                tx,
                ty,
                tile: Some(tile),
            })
            .collect(),
        dirty: rect,
    })
}
