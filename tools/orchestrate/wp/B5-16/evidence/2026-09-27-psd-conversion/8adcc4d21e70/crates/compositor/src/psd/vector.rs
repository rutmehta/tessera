//! Adobe vector masks, solid/gradient fills, and stroke descriptors.
use super::*;
use ::psd::metadata::{Descriptor as D, Value as V};
use ::vector as v;
fn object(class_id: &'static [u8]) -> D<'static> {
    D {
        name: String::new(),
        class_id,
        items: vec![],
    }
}
fn put<'a>(d: &mut D<'a>, key: &'a [u8], value: V<'a>) {
    if let Some((_, v)) = d.items.iter_mut().find(|(k, _)| *k == key) {
        *v = value;
    } else {
        d.items.push((key, value));
    }
}
fn num(d: &D<'_>, key: &[u8], default: f64) -> f64 {
    d.get(key).and_then(V::number).unwrap_or(default)
}
fn boolean(d: &D<'_>, key: &[u8], default: bool) -> bool {
    match d.get(key) {
        Some(V::Bool(v)) => *v,
        _ => default,
    }
}
fn enumeration<'a>(d: &D<'a>, key: &[u8]) -> Option<&'a [u8]> {
    match d.get(key) {
        Some(V::Enum { value, .. }) => Some(value),
        _ => None,
    }
}
fn unit(value: f64) -> V<'static> {
    V::Unit {
        unit: *b"#Pxl",
        value,
    }
}
fn en(type_id: &'static [u8], value: &'static [u8]) -> V<'static> {
    V::Enum { type_id, value }
}
fn descriptor(data: &[u8]) -> EngineResult<D<'_>> {
    super::adjustment_interop::descriptor(data)
}
fn encode(d: &D<'_>) -> Vec<u8> {
    let mut out = 16u32.to_be_bytes().to_vec();
    super::style_interop::descriptor(d, &mut out);
    out
}
fn color(d: &D<'_>) -> Option<[f32; 4]> {
    let c = d.get(b"Clr ").and_then(V::object).unwrap_or(d);
    Some([
        c.get(b"Rd  ")?.number()? as f32 / 255.,
        c.get(b"Grn ")?.number()? as f32 / 255.,
        c.get(b"Bl  ")?.number()? as f32 / 255.,
        1.,
    ])
}
fn color_object(c: [f32; 4]) -> D<'static> {
    let mut d = object(b"RGBC");
    for (k, c) in [(b"Rd  ".as_slice(), c[0]), (b"Grn ", c[1]), (b"Bl  ", c[2])] {
        put(&mut d, k, V::Double(f64::from(c) * 255.));
    }
    d
}
fn solid(c: [f32; 4]) -> D<'static> {
    let mut d = object(b"solidColorLayer");
    put(&mut d, b"Clr ", V::Object(color_object(c)));
    d
}
fn path(layer: &::psd::Layer, canvas: Extent) -> EngineResult<Option<(v::Path, u32)>> {
    let Some(block) = layer.info(b"vmsk").or_else(|| layer.info(b"vsms")) else {
        return Ok(None);
    };
    let mask = v::PsdVectorMask::decode(&block.data).map_err(error)?;
    let nonzero = mask
        .records
        .iter()
        .filter(|r| matches!(r.selector(), 0 | 3))
        .all(|r| r.0[6..8] == [0, 2]);
    let mut path = mask
        .path()
        .map_err(error)?
        .with_rule(if nonzero {
            v::FillRule::NonZero
        } else {
            v::FillRule::EvenOdd
        })
        .affine(v::Affine::scale_non_uniform(
            f64::from(canvas.width),
            f64::from(canvas.height),
        ));
    if (mask.flags & 1 != 0) ^ mask.initially_filled() {
        let rect = v::Shape::Rectangle {
            rect: v::Rect::new(0., 0., f64::from(canvas.width), f64::from(canvas.height)),
            radii: [0.; 4],
        }
        .path()
        .map_err(error)?;
        path = rect
            .boolean(&path, v::Operation::Subtract, 0.01)
            .map_err(error)?;
    }
    Ok(Some((path, mask.flags)))
}
pub(super) fn import_mask(
    layer: &::psd::Layer,
    canvas: Extent,
) -> EngineResult<Option<crate::VectorMask>> {
    let Some((path, flags)) = path(layer, canvas)? else {
        return Ok(None);
    };
    let params = layer.mask().map_err(error)?.and_then(|m| m.parameters);
    Ok(Some(crate::VectorMask {
        path,
        enabled: flags & 4 == 0,
        density: params.and_then(|p| p.vector_density).unwrap_or(255) as f32 / 255.,
        feather: params.and_then(|p| p.vector_feather).unwrap_or(0.) as f32,
    }))
}
fn write_path(
    path: &v::Path,
    flags: u32,
    layer: &mut ::psd::Layer,
    canvas: Extent,
) -> EngineResult<()> {
    let mut normalized = path.affine(v::Affine::scale_non_uniform(
        1. / f64::from(canvas.width),
        1. / f64::from(canvas.height),
    ));
    // PSD's per-subpath fill-rule field supports nonzero without flattening curves.
    let nonzero = normalized.fill_rule == v::FillRule::NonZero;
    normalized.fill_rule = v::FillRule::EvenOdd;
    let mut mask = v::PsdVectorMask::from_path(&normalized, flags).map_err(error)?;
    for record in &mut mask.records {
        if matches!(record.selector(), 0 | 3) {
            record.0[6..8].copy_from_slice(&(if nonzero { 2u16 } else { 1u16 }).to_be_bytes());
        }
    }
    let key = if layer.info(b"vsms").is_some() {
        *b"vsms"
    } else {
        *b"vmsk"
    };
    set_tag(layer, key, mask.encode());
    Ok(())
}
pub(super) fn export_mask(
    mask: Option<&crate::VectorMask>,
    layer: &mut ::psd::Layer,
    canvas: Extent,
    shape: bool,
) -> EngineResult<()> {
    if shape {
        return Ok(());
    }
    let before = import_mask(layer, canvas).ok().flatten();
    if mask == before.as_ref() {
        return Ok(());
    }
    let Some(mask) = mask else {
        if before.is_some() {
            layer
                .additional
                .retain(|b| !matches!(&b.key, b"vmsk" | b"vsms"));
        }
        return Ok(());
    };
    write_path(&mask.path, if mask.enabled { 0 } else { 4 }, layer, canvas)?;
    write_parameters(layer, mask.density, mask.feather)?;
    Ok(())
}
fn write_parameters(layer: &mut ::psd::Layer, density: f32, feather: f32) -> EngineResult<()> {
    if layer.mask_data.is_empty() {
        layer.mask_data = vec![0; 18];
        layer.mask_data[16] = 255;
    }
    let old = layer.mask().map_err(error)?.and_then(|m| m.parameters);
    let old_len = old
        .map(|p| {
            1 + usize::from(p.user_density.is_some())
                + 8 * usize::from(p.user_feather.is_some())
                + usize::from(p.vector_density.is_some())
                + 8 * usize::from(p.vector_feather.is_some())
        })
        .unwrap_or(0);
    let mut params = vec![12 | old.map(|p| p.flags & 3).unwrap_or(0)];
    if let Some(v) = old.and_then(|p| p.user_density) {
        params.push(v);
    }
    if let Some(v) = old.and_then(|p| p.user_feather) {
        params.extend(v.to_be_bytes());
    }
    params.push(crate::raster::quantize_u8(density));
    params.extend(f64::from(feather).to_be_bytes());
    layer.mask_data.splice(18..18 + old_len, params);
    layer.mask_data[17] |= 16;
    Ok(())
}
fn gradient(d: &D<'_>, canvas: Extent) -> EngineResult<v::Fill> {
    let g = d
        .get(b"Grad")
        .and_then(V::object)
        .ok_or_else(|| error("missing gradient"))?;
    let Some(V::List(colors)) = g.get(b"Clrs") else {
        return Err(error("missing gradient stops"));
    };
    let mut stops = Vec::new();
    for item in colors {
        let s = item
            .object()
            .ok_or_else(|| error("invalid gradient stop"))?;
        stops.push(v::Stop {
            position: num(s, b"Lctn", 0.) / 4096.,
            color: color(s).ok_or_else(|| error("non-RGB gradient color"))?,
        });
    }
    if let Some(V::List(alpha)) = g.get(b"Trns") {
        let alpha: Vec<_> = alpha
            .iter()
            .filter_map(V::object)
            .map(|s| {
                (
                    num(s, b"Lctn", 0.) / 4096.,
                    num(s, b"Opct", 100.) as f32 / 100.,
                )
            })
            .collect();
        // Adobe stores independent RGB and alpha stop positions. The native
        // model shares one stop list, so merge the union and interpolate RGB
        // at every alpha-only stop rather than dropping its opacity transition.
        let colors = stops.clone();
        let mut positions: Vec<_> = colors
            .iter()
            .map(|s| s.position)
            .chain(alpha.iter().map(|s| s.0))
            .collect();
        positions.sort_by(f64::total_cmp);
        positions.dedup();
        stops = positions
            .into_iter()
            .map(|position| {
                let mut color = color_at(&colors, position);
                color[3] = alpha_at(&alpha, position);
                v::Stop { position, color }
            })
            .collect();
    }
    let kind = match enumeration(d, b"Type").unwrap_or(b"Lnr ") {
        b"Lnr " => v::GradientKind::Linear,
        b"Rdl " => v::GradientKind::Radial,
        b"Angl" => v::GradientKind::Angle,
        b"Rflc" => v::GradientKind::Reflected,
        b"Dmnd" => v::GradientKind::Diamond,
        _ => return Err(error("unsupported gradient kind")),
    };
    let angle = num(d, b"Angl", 0.).to_radians();
    let length = f64::from(canvas.width) * num(d, b"Scl ", 100.) / 100.;
    let offset = d.get(b"Ofst").and_then(V::object);
    let center = v::Point::new(
        f64::from(canvas.width) * (0.5 + offset.map(|o| num(o, b"Hrzn", 0.)).unwrap_or(0.) / 100.),
        f64::from(canvas.height) * (0.5 + offset.map(|o| num(o, b"Vrtc", 0.)).unwrap_or(0.) / 100.),
    );
    let axis = v::Vec2::new(angle.cos() * length, -angle.sin() * length);
    let (start, end) = if matches!(kind, v::GradientKind::Linear) {
        (center - axis / 2., center + axis / 2.)
    } else {
        (center, center + axis)
    };
    if boolean(d, b"Rvrs", false) {
        for s in &mut stops {
            s.position = 1. - s.position;
        }
        stops.reverse();
    }
    Ok(v::Fill::Gradient(
        v::Gradient::new(kind, start, end, stops, boolean(d, b"Dthr", false)).map_err(error)?,
    ))
}
fn color_at(stops: &[v::Stop], position: f64) -> [f32; 4] {
    let Some(first) = stops.first() else {
        return [0.; 4];
    };
    if position <= first.position {
        return first.color;
    }
    for pair in stops.windows(2) {
        if position <= pair[1].position {
            let t = ((position - pair[0].position)
                / (pair[1].position - pair[0].position).max(f64::EPSILON))
                as f32;
            return std::array::from_fn(|i| {
                pair[0].color[i] + (pair[1].color[i] - pair[0].color[i]) * t
            });
        }
    }
    stops.last().unwrap().color
}
pub(super) fn needs_shape_mask(layer: &::psd::Layer) -> bool {
    let disabled = layer
        .info(b"vmsk")
        .or_else(|| layer.info(b"vsms"))
        .and_then(|b| v::PsdVectorMask::decode(&b.data).ok())
        .is_some_and(|m| m.flags & 4 != 0);
    let params = layer.mask().ok().flatten().and_then(|m| m.parameters);
    disabled
        || params.is_some_and(|p| {
            p.vector_density.is_some_and(|v| v != 255) || p.vector_feather.is_some_and(|v| v != 0.)
        })
}
fn alpha_at(stops: &[(f64, f32)], x: f64) -> f32 {
    if stops.is_empty() {
        return 1.;
    }
    if x <= stops[0].0 {
        return stops[0].1;
    }
    for p in stops.windows(2) {
        if x <= p[1].0 {
            let t = ((x - p[0].0) / (p[1].0 - p[0].0).max(f64::EPSILON)) as f32;
            return p[0].1 + (p[1].1 - p[0].1) * t;
        }
    }
    stops.last().unwrap().1
}
fn fill(layer: &::psd::Layer, canvas: Extent) -> EngineResult<Option<v::Fill>> {
    if let Some(b) = layer.info(b"SoCo") {
        return Ok(Some(v::Fill::Solid(
            color(&descriptor(&b.data)?).ok_or_else(|| error("non-RGB solid fill"))?,
        )));
    }
    if let Some(b) = layer.info(b"GdFl") {
        return Ok(Some(gradient(&descriptor(&b.data)?, canvas)?));
    }
    Ok(None)
}
fn stroke(layer: &::psd::Layer, canvas: Extent) -> EngineResult<Option<(v::Stroke, v::Fill)>> {
    let Some(b) = layer.info(b"vstk") else {
        return Ok(None);
    };
    let d = descriptor(&b.data)?;
    if !boolean(&d, b"strokeEnabled", true) {
        return Ok(None);
    }
    let content = d
        .get(b"strokeStyleContent")
        .and_then(V::object)
        .ok_or_else(|| error("stroke content missing"))?;
    let fill = if let Some(c) = color(content) {
        v::Fill::Solid(c)
    } else {
        gradient(content, canvas)?
    };
    let width = num(&d, b"strokeStyleLineWidth", 1.);
    let s = v::Stroke {
        width,
        alignment: match enumeration(&d, b"strokeStyleLineAlignment") {
            Some(b"strokeStyleAlignInside") => v::Alignment::Inside,
            Some(b"strokeStyleAlignOutside") => v::Alignment::Outside,
            _ => v::Alignment::Center,
        },
        cap: match enumeration(&d, b"strokeStyleLineCapType") {
            Some(b"strokeStyleRoundCap") => v::LineCap::Round,
            Some(b"strokeStyleSquareCap") => v::LineCap::Square,
            _ => v::LineCap::Butt,
        },
        join: match enumeration(&d, b"strokeStyleLineJoinType") {
            Some(b"strokeStyleRoundJoin") => v::LineJoin::Round,
            Some(b"strokeStyleBevelJoin") => v::LineJoin::Bevel,
            _ => v::LineJoin::Miter,
        },
        miter_limit: num(&d, b"strokeStyleMiterLimit", 4.),
        dash_offset: num(&d, b"strokeStyleLineDashOffset", 0.) * width,
        dashes: match d.get(b"strokeStyleLineDashSet") {
            Some(V::List(v)) => v.iter().filter_map(V::number).map(|n| n * width).collect(),
            _ => vec![],
        },
    };
    Ok(Some((s, fill)))
}
fn live_shape(layer: &::psd::Layer) -> Option<v::Shape> {
    let data = &layer.info(b"vogk")?.data;
    if data.get(..4) != Some(&1u32.to_be_bytes()) {
        return None;
    }
    let d = descriptor(data.get(4..)?).ok()?;
    let V::List(items) = d.get(b"keyDescriptorList")? else {
        return None;
    };
    let item = items.first()?.object()?;
    if boolean(item, b"keyShapeInvalidated", false) {
        return None;
    }
    let bbox = item.get(b"keyOriginShapeBBox")?.object()?;
    let rect = v::Rect::new(
        num(bbox, b"Left", 0.),
        num(bbox, b"Top ", 0.),
        num(bbox, b"Rght", 0.),
        num(bbox, b"Btom", 0.),
    );
    match num(item, b"keyOriginType", 0.) as i32 {
        1 | 2 => {
            let r = item.get(b"keyOriginRRectRadii").and_then(V::object);
            Some(v::Shape::Rectangle {
                rect,
                radii: [
                    b"topLeft".as_slice(),
                    b"topRight",
                    b"bottomRight",
                    b"bottomLeft",
                ]
                .map(|k| r.map(|r| num(r, k, 0.)).unwrap_or(0.)),
            })
        }
        5 => Some(v::Shape::Ellipse {
            center: rect.center(),
            radii: v::Vec2::new(rect.width() / 2., rect.height() / 2.),
        }),
        _ => None,
    }
}
pub(super) fn import_shape(
    layer: &::psd::Layer,
    canvas: Extent,
) -> EngineResult<Option<LayerKind>> {
    if let Some(shape) = restore_shape(layer) {
        return Ok(Some(shape));
    }
    if ![b"SoCo", b"GdFl", b"vstk"]
        .iter()
        .any(|k| layer.info(k).is_some())
    {
        return Ok(None);
    }
    // Unsupported sources stay raster proxies with their exact tagged data.
    let Some((mut path, _)) = path(layer, canvas).ok().flatten() else {
        return Ok(None);
    };
    let Ok(fill) = fill(layer, canvas) else {
        return Ok(None);
    };
    let Ok(stroke) = stroke(layer, canvas) else {
        return Ok(None);
    };
    let masked = needs_shape_mask(layer);
    if masked {
        // A disabled or reduced-density shape mask reveals the fill outside
        // its path. Keep a live canvas paint source and a separate live mask.
        // Stroke+nondefault-mask semantics remain an explicit raster proxy.
        if stroke.is_some() {
            return Ok(None);
        }
        path = v::Shape::Rectangle {
            rect: v::Rect::new(0., 0., f64::from(canvas.width), f64::from(canvas.height)),
            radii: [0.; 4],
        }
        .path()
        .map_err(error)?;
    }
    let fill_enabled = layer
        .info(b"vstk")
        .and_then(|b| descriptor(&b.data).ok())
        .map(|d| boolean(&d, b"fillEnabled", true))
        .unwrap_or(true);
    Ok(Some(LayerKind::Shape {
        model: v::ShapeModel {
            path,
            fill: if fill_enabled { fill } else { None },
            stroke,
            live_shape: if masked { None } else { live_shape(layer) },
        },
        transform: crate::Affine::default(),
    }))
}
fn encode_fill(fill: &v::Fill, canvas: Extent) -> EngineResult<([u8; 4], D<'static>)> {
    match fill {
        v::Fill::Solid(c) => Ok((*b"SoCo", solid(*c))),
        v::Fill::Pattern(_) => Err(error("PSD pattern shape fill is unsupported")),
        v::Fill::Gradient(g) => {
            let mut d = object(b"gradientLayer");
            let mut gradient = object(b"Grdn");
            put(&mut gradient, b"Nm  ", V::Text("Custom".into()));
            put(&mut gradient, b"GrdF", en(b"GrdF", b"CstS"));
            put(&mut gradient, b"Intr", V::Double(4096.));
            let mut colors = Vec::new();
            let mut alphas = Vec::new();
            for s in &g.stops {
                let mut c = object(b"Clrt");
                put(&mut c, b"Clr ", V::Object(color_object(s.color)));
                put(&mut c, b"Type", en(b"Clry", b"UsrS"));
                put(
                    &mut c,
                    b"Lctn",
                    V::Integer((s.position * 4096.).round() as i32),
                );
                put(&mut c, b"Mdpn", V::Integer(50));
                colors.push(V::Object(c));
                let mut a = object(b"TrnS");
                put(
                    &mut a,
                    b"Opct",
                    V::Unit {
                        unit: *b"#Prc",
                        value: f64::from(s.color[3]) * 100.,
                    },
                );
                put(
                    &mut a,
                    b"Lctn",
                    V::Integer((s.position * 4096.).round() as i32),
                );
                put(&mut a, b"Mdpn", V::Integer(50));
                alphas.push(V::Object(a));
            }
            put(&mut gradient, b"Clrs", V::List(colors));
            put(&mut gradient, b"Trns", V::List(alphas));
            put(&mut d, b"Grad", V::Object(gradient));
            let kind = match g.kind {
                v::GradientKind::Linear => b"Lnr ",
                v::GradientKind::Radial => b"Rdl ",
                v::GradientKind::Angle => b"Angl",
                v::GradientKind::Reflected => b"Rflc",
                v::GradientKind::Diamond => b"Dmnd",
            };
            put(&mut d, b"Type", en(b"GrdT", kind));
            let axis = g.end - g.start;
            put(
                &mut d,
                b"Angl",
                V::Unit {
                    unit: *b"#Ang",
                    value: (-axis.y).atan2(axis.x).to_degrees(),
                },
            );
            put(
                &mut d,
                b"Scl ",
                V::Unit {
                    unit: *b"#Prc",
                    value: axis.hypot() / f64::from(canvas.width) * 100.,
                },
            );
            put(&mut d, b"Dthr", V::Bool(g.dither));
            put(&mut d, b"Rvrs", V::Bool(false));
            put(&mut d, b"Algn", V::Bool(false));
            let center = if matches!(g.kind, v::GradientKind::Linear) {
                g.start + axis / 2.
            } else {
                g.start
            };
            let mut off = object(b"Pnt ");
            put(
                &mut off,
                b"Hrzn",
                V::Double((center.x / f64::from(canvas.width) - 0.5) * 100.),
            );
            put(
                &mut off,
                b"Vrtc",
                V::Double((center.y / f64::from(canvas.height) - 0.5) * 100.),
            );
            put(&mut d, b"Ofst", V::Object(off));
            Ok((*b"GdFl", d))
        }
    }
}
fn merge<'a>(old: &mut D<'a>, new: D<'a>) {
    for (k, value) in new.items {
        if let V::Object(child) = value {
            if let Some((_, V::Object(existing))) = old.items.iter_mut().find(|(key, _)| *key == k)
            {
                merge(existing, child);
                continue;
            }
            put(old, k, V::Object(child));
        } else {
            put(old, k, value);
        }
    }
}
fn write_fill(fill: &v::Fill, layer: &mut ::psd::Layer, canvas: Extent) -> EngineResult<()> {
    let (key, d) = encode_fill(fill, canvas)?;
    let old = layer.info(&key).map(|b| b.data.clone());
    let mut merged = old
        .as_deref()
        .map(descriptor)
        .transpose()?
        .unwrap_or_else(|| object(b"null"));
    merge(&mut merged, d);
    let data = encode(&merged);
    layer
        .additional
        .retain(|b| !matches!(&b.key, b"SoCo" | b"GdFl") || b.key == key);
    set_tag(layer, key, data);
    Ok(())
}
fn write_stroke(
    model: &v::ShapeModel,
    layer: &mut ::psd::Layer,
    canvas: Extent,
) -> EngineResult<()> {
    let old = layer.info(b"vstk").map(|b| b.data.clone());
    let mut d = old
        .as_deref()
        .map(descriptor)
        .transpose()?
        .unwrap_or_else(|| object(b"strokeStyle"));
    put(&mut d, b"strokeStyleVersion", V::Integer(2));
    put(&mut d, b"strokeEnabled", V::Bool(model.stroke.is_some()));
    put(&mut d, b"fillEnabled", V::Bool(model.fill.is_some()));
    if let Some((s, fill)) = &model.stroke {
        put(&mut d, b"strokeStyleLineWidth", unit(s.width));
        put(&mut d, b"strokeStyleMiterLimit", V::Double(s.miter_limit));
        put(
            &mut d,
            b"strokeStyleLineDashOffset",
            V::Double(s.dash_offset / s.width.max(f64::EPSILON)),
        );
        put(
            &mut d,
            b"strokeStyleLineDashSet",
            V::List(
                s.dashes
                    .iter()
                    .map(|n| V::Double(n / s.width.max(f64::EPSILON)))
                    .collect(),
            ),
        );
        put(
            &mut d,
            b"strokeStyleLineAlignment",
            en(
                b"strokeStyleLineAlignment",
                match s.alignment {
                    v::Alignment::Inside => b"strokeStyleAlignInside",
                    v::Alignment::Center => b"strokeStyleAlignCenter",
                    v::Alignment::Outside => b"strokeStyleAlignOutside",
                },
            ),
        );
        put(
            &mut d,
            b"strokeStyleLineCapType",
            en(
                b"strokeStyleLineCapType",
                match s.cap {
                    v::LineCap::Round => b"strokeStyleRoundCap",
                    v::LineCap::Square => b"strokeStyleSquareCap",
                    _ => b"strokeStyleButtCap",
                },
            ),
        );
        put(
            &mut d,
            b"strokeStyleLineJoinType",
            en(
                b"strokeStyleLineJoinType",
                match s.join {
                    v::LineJoin::Round => b"strokeStyleRoundJoin",
                    v::LineJoin::Bevel => b"strokeStyleBevelJoin",
                    _ => b"strokeStyleMiterJoin",
                },
            ),
        );
        let (_, content) = encode_fill(fill, canvas)?;
        let mut patch = object(b"strokeStyle");
        put(&mut patch, b"strokeStyleContent", V::Object(content));
        merge(&mut d, patch);
    }
    set_tag(layer, *b"vstk", encode(&d));
    Ok(())
}
fn write_live(shape: Option<&v::Shape>, layer: &mut ::psd::Layer) -> EngineResult<()> {
    let old = layer.info(b"vogk").map(|b| b.data.clone());
    let mut d = old
        .as_deref()
        .and_then(|b| b.get(4..))
        .map(descriptor)
        .transpose()?
        .unwrap_or_else(|| object(b"null"));
    let mut items = match d.get(b"keyDescriptorList") {
        Some(V::List(v)) => v.clone(),
        _ => vec![],
    };
    let mut item = items
        .first()
        .and_then(V::object)
        .cloned()
        .unwrap_or_else(|| object(b"null"));
    put(&mut item, b"keyOriginIndex", V::Integer(0));
    let bounds = match shape {
        Some(v::Shape::Rectangle { rect, radii }) => {
            put(
                &mut item,
                b"keyOriginType",
                V::Integer(if radii.iter().any(|v| *v != 0.) { 2 } else { 1 }),
            );
            let mut r = object(b"radii");
            put(&mut r, b"unitValueQuadVersion", V::Integer(1));
            for (k, value) in [
                b"topLeft".as_slice(),
                b"topRight",
                b"bottomRight",
                b"bottomLeft",
            ]
            .into_iter()
            .zip(radii)
            {
                put(&mut r, k, unit(*value));
            }
            put(&mut item, b"keyOriginRRectRadii", V::Object(r));
            Some(*rect)
        }
        Some(v::Shape::Ellipse { center, radii }) => {
            put(&mut item, b"keyOriginType", V::Integer(5));
            Some(v::Rect::new(
                center.x - radii.x,
                center.y - radii.y,
                center.x + radii.x,
                center.y + radii.y,
            ))
        }
        _ => None,
    };
    put(&mut item, b"keyShapeInvalidated", V::Bool(bounds.is_none()));
    if let Some(b) = bounds {
        let mut bbox = object(b"unitValueQuad");
        put(&mut bbox, b"unitValueQuadVersion", V::Integer(1));
        for (k, value) in [
            (b"Left".as_slice(), b.x0),
            (b"Top ", b.y0),
            (b"Rght", b.x1),
            (b"Btom", b.y1),
        ] {
            put(&mut bbox, k, unit(value));
        }
        put(&mut item, b"keyOriginShapeBBox", V::Object(bbox));
    }
    if items.is_empty() {
        items.push(V::Object(item));
    } else {
        items[0] = V::Object(item);
    }
    put(&mut d, b"keyDescriptorList", V::List(items));
    let mut data = 1u32.to_be_bytes().to_vec();
    data.extend(encode(&d));
    set_tag(layer, *b"vogk", data);
    Ok(())
}
pub(super) fn export_shape(
    model: &v::ShapeModel,
    transform: crate::Affine,
    layer: &mut ::psd::Layer,
    canvas: Extent,
) -> EngineResult<()> {
    if let Some(LayerKind::Shape {
        model: before,
        transform: old,
    }) = import_shape(layer, canvas)?
        && *model == before
        && transform == old
        && !needs_shape_mask(layer)
    {
        return Ok(());
    }
    let [a, c, e, b, d, f] = transform.m;
    let path = model.path.affine(v::Affine::new([a, b, c, d, e, f]));
    write_path(&path, 0, layer, canvas)?;
    if let Some(fill) = &model.fill {
        write_fill(fill, layer, canvas)?;
    }
    let mut standard = model.clone();
    // Uniform affine scaling maps exactly to Adobe's document-space stroke width.
    let scale_x = a.hypot(b);
    let scale_y = c.hypot(d);
    if (scale_x - scale_y).abs() < 1e-9
        && let Some((stroke, _)) = &mut standard.stroke
    {
        stroke.width *= scale_x;
        stroke.dash_offset *= scale_x;
        for dash in &mut stroke.dashes {
            *dash *= scale_x;
        }
    }
    write_stroke(&standard, layer, canvas)?;
    write_live(
        if transform == crate::Affine::default() {
            model.live_shape.as_ref()
        } else {
            None
        },
        layer,
    )?;
    // Private editable-source bridge, separate from Adobe's descriptors. The
    // digest prevents stale native controls from overriding external PSD edits.
    let bridge = ShapeBridge {
        version: 1,
        model: model.clone(),
        transform,
        fingerprint: shape_fingerprint(layer),
    };
    set_tag(layer, *b"tvSh", serde_json::to_vec(&bridge).map_err(error)?);
    Ok(())
}

// PSD exposes one vector path per shape. A second, independent vector mask is
// exported as a standard raster mask and this private bridge retains its live
// source plus the uncombined user-mask planes. Adobe applications see the raster.
#[derive(serde::Serialize, serde::Deserialize)]
struct MaskBridge {
    version: u32,
    vector: crate::VectorMask,
    mask_data: Vec<u8>,
    channels: Vec<(i16, Vec<u8>)>,
    fingerprint: [u8; 32],
}
pub(super) fn restore_bridge(
    layer: &::psd::Layer,
    canvas: Extent,
    depth: Depth,
) -> EngineResult<Option<(crate::VectorMask, Option<crate::Mask>)>> {
    let Some(tag) = layer.info(b"tvMk") else {
        return Ok(None);
    };
    if tag.data.len() > 64 << 20 {
        return Err(error("vector mask bridge exceeds limit"));
    }
    let bridge: MaskBridge = serde_json::from_slice(&tag.data).map_err(error)?;
    if bridge.version != 1 {
        return Err(error("unsupported vector mask bridge version"));
    }
    if bridge.fingerprint != mask_fingerprint(layer) {
        return Ok(None);
    }
    crate::text_vector::validate_mask(&bridge.vector)?;
    let mut original = layer.clone();
    original.mask_data = bridge.mask_data;
    original.channels.retain(|c| c.id != -2 && c.id != -3);
    original
        .channels
        .extend(bridge.channels.into_iter().map(|(id, data)| Channel {
            id,
            data,
            compression: Compression::Raw,
        }));
    Ok(Some((
        bridge.vector,
        super::import_mask(&original, canvas, depth)?,
    )))
}
pub(super) fn export_bridge(
    node: &Layer,
    layer: &mut ::psd::Layer,
    canvas: Extent,
    depth: Depth,
    cancel: &engine_api::jobs::CancellationToken,
) -> EngineResult<()> {
    cancel.check()?;
    if !matches!(node.kind, LayerKind::Shape { .. }) {
        return Ok(());
    }
    let Some(mask) = &node.vector_mask else {
        layer.additional.retain(|b| b.key != *b"tvMk");
        return Ok(());
    };
    let mut bridge = MaskBridge {
        version: 1,
        fingerprint: [0; 32],
        vector: mask.clone(),
        mask_data: layer.mask_data.clone(),
        channels: layer
            .channels
            .iter()
            .filter(|c| c.id == -2 || c.id == -3)
            .map(|c| (c.id, c.data.clone()))
            .collect(),
    };
    let mut raster = Raster::new(canvas, 1, Depth::F32, 1.);
    let comp = crate::Compositor::new(0);
    let tile_size = engine_api::tile::TILE_SIZE;
    let (cols, rows) = canvas.tile_grid(tile_size);
    for y in 0..rows {
        cancel.check()?;
        for x in 0..cols {
            cancel.check()?;
            let tile = if mask.enabled {
                Some(comp.vector_mask_tile(
                    node,
                    canvas,
                    Depth::F32,
                    engine_api::tile::TileCoord::new(0, x, y),
                )?)
            } else {
                None
            };
            let width = tile
                .as_ref()
                .map(|t| t.layout().extent.width)
                .unwrap_or((canvas.width - x * tile_size).min(tile_size));
            let height = (canvas.height - y * tile_size).min(tile_size);
            let samples = tile.as_ref().map(|t| t.samples::<f32>()).transpose()?;
            raster.edit_region(
                Rect::new(
                    i64::from(x * tile_size),
                    i64::from(y * tile_size),
                    i64::from(x * tile_size + width),
                    i64::from(y * tile_size + height),
                ),
                0,
                |px, py, p| {
                    let coverage = samples
                        .map(|v| v[((py - y * tile_size) * width + px - x * tile_size) as usize])
                        .unwrap_or(1.);
                    let user = node
                        .mask
                        .as_ref()
                        .filter(|m| m.enabled)
                        .map(|m| 1. - m.density * (1. - m.raster.pixel(px, py)[0]))
                        .unwrap_or(1.);
                    p[0] = coverage * user;
                },
            )?;
            cancel.check()?;
        }
    }
    // Reset bounds so the combined plane covers the full document.
    layer.mask_data.clear();
    layer.channels.retain(|c| c.id != -2 && c.id != -3);
    super::export_mask(
        Some(&crate::Mask {
            raster,
            enabled: true,
            density: 1.,
            feather: 0.,
        }),
        layer,
        canvas,
        depth,
        cancel,
    )?;
    cancel.check()?;
    bridge.fingerprint = mask_fingerprint(layer);
    set_tag(layer, *b"tvMk", serde_json::to_vec(&bridge).map_err(error)?);
    cancel.check()
}

#[derive(serde::Serialize, serde::Deserialize)]
struct ShapeBridge {
    version: u32,
    model: v::ShapeModel,
    transform: crate::Affine,
    fingerprint: [u8; 32],
}
fn shape_fingerprint(layer: &::psd::Layer) -> [u8; 32] {
    let mut hash = blake3::Hasher::new();
    for key in [b"vmsk", b"vsms", b"vogk", b"SoCo", b"GdFl", b"vstk"] {
        if let Some(b) = layer.info(key) {
            hash.update(key);
            hash.update(&(b.data.len() as u64).to_be_bytes());
            hash.update(&b.data);
        }
    }
    *hash.finalize().as_bytes()
}
fn restore_shape(layer: &::psd::Layer) -> Option<LayerKind> {
    let tag = layer.info(b"tvSh")?;
    if tag.data.len() > 16 << 20 {
        return None;
    }
    let bridge: ShapeBridge = serde_json::from_slice(&tag.data).ok()?;
    if bridge.version != 1 || bridge.fingerprint != shape_fingerprint(layer) {
        return None;
    }
    crate::text_vector::validate_shape(&bridge.model, bridge.transform).ok()?;
    Some(LayerKind::Shape {
        model: bridge.model,
        transform: bridge.transform,
    })
}

fn mask_fingerprint(layer: &::psd::Layer) -> [u8; 32] {
    let mut hash = blake3::Hasher::new();
    hash.update(&layer.mask_data);
    for channel in layer.channels.iter().filter(|c| c.id == -2 || c.id == -3) {
        hash.update(&channel.id.to_be_bytes());
        hash.update(&(channel.data.len() as u64).to_be_bytes());
        hash.update(&channel.data);
    }
    *hash.finalize().as_bytes()
}
