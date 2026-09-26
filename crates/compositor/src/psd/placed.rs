//! Standard SoLd/PlLd Action Descriptors and embedded liFD PSD originals.
//! Trnf is TL, TR, BR, BL in parent pixels; Sz is the untransformed size.
//! Layout cross-checked against ag-psd src/additionalInfo.ts (SoLd) and
//! the psd crate's linked-file parser, not private JSON or pseudo-records.
use super::*;
use ::psd::metadata::{Descriptor as D, Value as V};

fn object(class_id: &'static [u8], items: Vec<(&'static [u8], V<'static>)>) -> D<'static> {
    D {
        name: String::new(),
        class_id,
        items,
    }
}

pub(super) fn import(
    layer: &::psd::Layer,
    global: &[::psd::AdditionalInfo],
) -> EngineResult<Option<crate::SmartObject>> {
    let Some(block) = [b"SoLd", b"PlLd", b"SoLE"]
        .iter()
        .find_map(|k| layer.info(k))
    else {
        return Ok(None);
    };
    // Unsupported descriptors stay on the existing rendered-proxy path.
    let Ok(parsed) = ::psd::metadata::parse_smart_object(&block.data) else {
        return Ok(None);
    };
    let d = &parsed.descriptor;
    if d.get(b"filterFX").is_some() || d.get(b"quiltWarp").is_some() {
        return Ok(None);
    }
    if let Some(warp) = d.get(b"warp").and_then(V::object)
        && !matches!(
            warp.get(b"warpStyle"),
            Some(V::Enum {
                value: b"warpNone",
                ..
            })
        )
    {
        return Ok(None);
    }
    let Some(V::List(points)) = d.get(b"Trnf") else {
        return Ok(None);
    };
    if points.len() != 8 {
        return Ok(None);
    }
    let q: Option<Vec<f64>> = points.iter().map(V::number).collect();
    let Some(q) = q.filter(|q| q.iter().all(|v| v.is_finite())) else {
        return Ok(None);
    };
    if let Some(nonaffine) = d.get(b"nonAffineTransform")
        && nonaffine != &V::List(points.clone())
    {
        return Ok(None);
    }
    let Some(size) = d.get(b"Sz  ").and_then(V::object) else {
        return Ok(None);
    };
    let (Some(w), Some(h)) = (
        size.get(b"Wdth").and_then(V::number),
        size.get(b"Hght").and_then(V::number),
    ) else {
        return Ok(None);
    };
    if !w.is_finite() || !h.is_finite() || w <= 0. || h <= 0. {
        return Ok(None);
    }
    let affine = crate::Affine {
        m: [
            (q[2] - q[0]) / w,
            (q[6] - q[0]) / h,
            q[0],
            (q[3] - q[1]) / w,
            (q[7] - q[1]) / h,
            q[1],
        ],
    };
    let br = affine.apply(w, h);
    if affine.inverse().is_none() || (br.0 - q[4]).abs() > 1e-7 || (br.1 - q[5]).abs() > 1e-7 {
        return Ok(None);
    }
    let Some(V::Text(id)) = d.get(b"Idnt") else {
        return Ok(None);
    };
    for b in global
        .iter()
        .chain(&layer.additional)
        .filter(|b| matches!(&b.key, b"lnkD" | b"lnk2" | b"lnk3"))
    {
        let Ok(files) = ::psd::metadata::parse_linked_files(&b.data) else {
            continue;
        };
        for file in files {
            if file.unique_id != id.as_bytes() || file.file_type != *b"8BPS" {
                continue;
            }
            let mut source = PsdDocument::read(file.original).map_err(error)?;
            if f64::from(source.width) != w || f64::from(source.height) != h {
                return Err(error("placed source size disagrees with descriptor"));
            }
            // Use the embedded source's merged pixels: never recursively open
            // nested linked files, external paths, or apply transforms to a proxy.
            source.layer_section.layers.clear();
            source.layer_section.additional.clear();
            let child = from_psd(&source)?.state;
            return Ok(Some(crate::SmartObject::new(child, affine)));
        }
    }
    Ok(None)
}

pub(super) fn append_linked(
    global: &mut Vec<::psd::AdditionalInfo>,
    data: &[u8],
) -> EngineResult<()> {
    let mut ids = std::collections::BTreeSet::new();
    for b in global
        .iter()
        .filter(|b| matches!(&b.key, b"lnkD" | b"lnk2" | b"lnk3"))
    {
        if let Ok(files) = ::psd::metadata::parse_linked_files(&b.data) {
            ids.extend(files.into_iter().map(|f| f.unique_id.to_vec()));
        }
    }
    let files = ::psd::metadata::parse_linked_files(data).map_err(error)?;
    let mut offset = 0;
    let mut unique = Vec::new();
    for file in files {
        let n = u64::from_be_bytes(data[offset..offset + 8].try_into().unwrap()) as usize;
        let end = offset + 8 + n + (4 - n % 4) % 4;
        if ids.insert(file.unique_id.to_vec()) {
            unique.extend_from_slice(&data[offset..end]);
        }
        offset = end;
    }
    if !unique.is_empty() {
        if let Some(block) = global
            .iter_mut()
            .find(|b| b.key == *b"lnk2" && ::psd::metadata::parse_linked_files(&b.data).is_ok())
        {
            block.data.extend(unique);
        } else {
            global.push(::psd::AdditionalInfo {
                signature: *b"8BIM",
                key: *b"lnk2",
                data: unique,
            });
        }
    }
    Ok(())
}

fn render(state: DocState, depth: Depth) -> EngineResult<Raster> {
    let e = state.canvas;
    let (_, rgba) =
        crate::Compositor::new(64 << 20).render_level_rgba(&crate::Document::new(state), 0)?;
    let mut raster = Raster::new(e, 4, depth, 0.);
    raster.edit_region(Rect::of_extent(e), 1, |x, y, p| {
        let i = (y as usize * e.width as usize + x as usize) * 4;
        p.copy_from_slice(&rgba[i..i + 4]);
    })?;
    Ok(raster)
}

pub(super) fn export(
    so: &crate::SmartObject,
    layer: &mut ::psd::Layer,
    canvas: Extent,
    depth: Depth,
) -> EngineResult<Raster> {
    if so.filters.iter().any(|f| f.enabled) {
        return Err(error(
            "smart filters and TransformOp stages are native-only; rasterize explicitly for PSD",
        ));
    }
    if so.transform.m.iter().any(|v| !v.is_finite()) || so.transform.inverse().is_none() {
        return Err(error("invalid placed affine"));
    }
    let e = so.state.canvas;
    let raster = render((*so.state).clone(), so.state.depth)?;
    let mut flat = DocState::new(e, so.state.depth);
    flat.ppi = so.state.ppi;
    flat.profile = so.state.profile.clone();
    flat.root.push(Arc::new(Layer::new(
        "Embedded source",
        LayerKind::Pixel(raster),
    )));
    let bytes = to_psd(&crate::Document::new(flat))?
        .write()
        .map_err(error)?;
    let digest = blake3::hash(&bytes).to_hex().to_string();
    let id = format!(
        "{}-{}-{}-{}-{}",
        &digest[..8],
        &digest[8..12],
        &digest[12..16],
        &digest[16..20],
        &digest[20..32]
    );
    let corners = [
        (0., 0.),
        (f64::from(e.width), 0.),
        (f64::from(e.width), f64::from(e.height)),
        (0., f64::from(e.height)),
    ];
    let q: Vec<V<'static>> = corners
        .into_iter()
        .flat_map(|(x, y)| {
            let (x, y) = so.transform.apply(x, y);
            [V::Double(x), V::Double(y)]
        })
        .collect();
    let fraction = || {
        V::Object(object(
            b"null",
            vec![
                (b"numerator", V::Integer(0)),
                (b"denominator", V::Integer(600)),
            ],
        ))
    };
    let d = object(
        b"null",
        vec![
            (b"Idnt", V::Text(id.clone())),
            (b"placed", V::Text(id.clone())),
            (b"PgNm", V::Integer(1)),
            (b"totalPages", V::Integer(1)),
            (b"frameStep", fraction()),
            (b"duration", fraction()),
            (b"frameCount", V::Integer(0)),
            (b"Annt", V::Integer(16)),
            (b"Type", V::Integer(2)),
            (b"Trnf", V::List(q.clone())),
            (b"nonAffineTransform", V::List(q)),
            (
                b"Sz  ",
                V::Object(object(
                    b"Pnt ",
                    vec![
                        (b"Wdth", V::Double(e.width.into())),
                        (b"Hght", V::Double(e.height.into())),
                    ],
                )),
            ),
            (
                b"Rslt",
                V::Unit {
                    unit: *b"#Rsl",
                    value: f64::from(so.state.ppi),
                },
            ),
            (
                b"warp",
                V::Object(object(
                    b"warp",
                    vec![
                        (
                            b"warpStyle",
                            V::Enum {
                                type_id: b"warpStyle",
                                value: b"warpNone",
                            },
                        ),
                        (b"warpValue", V::Double(0.)),
                        (b"warpPerspective", V::Double(0.)),
                        (b"warpPerspectiveOther", V::Double(0.)),
                        (
                            b"warpRotate",
                            V::Enum {
                                type_id: b"Ornt",
                                value: b"Hrzn",
                            },
                        ),
                        (
                            b"bounds",
                            V::Object(object(
                                b"Rctn",
                                vec![
                                    (b"Top ", V::Double(0.)),
                                    (b"Left", V::Double(0.)),
                                    (b"Btom", V::Double(e.height.into())),
                                    (b"Rght", V::Double(e.width.into())),
                                ],
                            )),
                        ),
                        (b"uOrder", V::Integer(4)),
                        (b"vOrder", V::Integer(4)),
                    ],
                )),
            ),
        ],
    );
    let keys: Vec<[u8; 4]> = [*b"SoLd", *b"PlLd", *b"SoLE"]
        .into_iter()
        .filter(|key| layer.info(key).is_some())
        .collect();
    let keys = if keys.is_empty() {
        vec![*b"SoLd"]
    } else {
        keys
    };
    for key in keys {
        let mut merged = layer
            .info(&key)
            .and_then(|b| ::psd::metadata::parse_smart_object(&b.data).ok())
            .map(|p| p.descriptor)
            .unwrap_or_else(|| d.clone());
        for (key, value) in &d.items {
            if let Some((_, old)) = merged.items.iter_mut().find(|(k, _)| k == key) {
                *old = value.clone();
            } else {
                merged.items.push((key, value.clone()));
            }
        }
        let mut data = [
            b"soLD".as_slice(),
            &4u32.to_be_bytes(),
            &16u32.to_be_bytes(),
        ]
        .concat();
        super::style_interop::descriptor(&merged, &mut data);
        set_tag(layer, key, data);
    }
    // Do not leave a second, legacy placement with stale corners or source ID.
    // Descriptor-based SoLd/PlLd is the authoritative replacement on edited export.
    layer.additional.retain(|b| b.key != *b"plLd");
    let mut entry = [
        b"liFD".as_slice(),
        &1u32.to_be_bytes(),
        &[id.len() as u8],
        id.as_bytes(),
    ]
    .concat();
    let filename: Vec<u16> = "source.psd".encode_utf16().collect();
    entry.extend((filename.len() as u32).to_be_bytes());
    for v in filename {
        entry.extend(v.to_be_bytes());
    }
    entry.extend(b"8BPS8BIM");
    entry.extend((bytes.len() as u64).to_be_bytes());
    entry.push(0); // no open-parameters descriptor
    entry.extend(bytes);
    let mut linked = (entry.len() as u64).to_be_bytes().to_vec();
    linked.extend(&entry);
    linked.resize(linked.len() + (4 - entry.len() % 4) % 4, 0);
    set_tag(layer, *b"lnk2", linked); // lifted to document additional-info on export
    let mut parent = DocState::new(canvas, depth);
    parent.root.push(Arc::new(Layer::new(
        "Proxy",
        LayerKind::SmartObject(so.clone()),
    )));
    render(parent, depth)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn retains_unknown_descriptor_fields_on_affine_export() {
        let e = Extent::new(2, 2);
        let so = crate::SmartObject::new(DocState::new(e, Depth::U8), crate::Affine::IDENTITY);
        let mut layer = ::psd::Layer::default();
        export(&so, &mut layer, e, Depth::U8).unwrap();
        let mut d = ::psd::metadata::parse_smart_object(&layer.info(b"SoLd").unwrap().data)
            .unwrap()
            .descriptor;
        d.items.push((b"futureField", V::Integer(42)));
        let mut data = [
            b"soLD".as_slice(),
            &4u32.to_be_bytes(),
            &16u32.to_be_bytes(),
        ]
        .concat();
        super::super::style_interop::descriptor(&d, &mut data);
        set_tag(&mut layer, *b"SoLd", data);
        export(&so, &mut layer, e, Depth::U8).unwrap();
        let d = ::psd::metadata::parse_smart_object(&layer.info(b"SoLd").unwrap().data)
            .unwrap()
            .descriptor;
        assert_eq!(d.get(b"futureField"), Some(&V::Integer(42)));
    }
}
