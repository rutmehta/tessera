//! Editable TySh adapter. Original descriptors retain fields we do not model.
use super::*;
use ::psd::metadata::{Descriptor as D, Value as V};
pub(super) fn import(layer: &::psd::Layer) -> Option<LayerKind> {
    let source = ::psd::metadata::parse_text(&layer.info(b"TySh")?.data).ok()?;
    let text = typography::import_tysh(&source).ok()?;
    if !text.warnings.is_empty() && restore_model(layer).is_none() {
        return None;
    }
    let [a, b, c, d, e, f] = text.transform;
    Some(LayerKind::Text {
        model: restore_model(layer).unwrap_or(text.model),
        transform: crate::Affine {
            m: [a, c, e, b, d, f],
        },
    })
}
fn put<'a>(d: &mut D<'a>, key: &'a [u8], value: V<'a>) {
    if let Some((_, v)) = d.items.iter_mut().find(|(k, _)| *k == key) {
        *v = value;
    } else {
        d.items.push((key, value));
    }
}
fn empty<'a>() -> D<'a> {
    D {
        name: String::new(),
        class_id: b"TxLr",
        items: Vec::new(),
    }
}
pub(super) fn export(
    model: &typography::TextModel,
    transform: crate::Affine,
    layer: &mut ::psd::Layer,
) -> EngineResult<()> {
    if let Some(LayerKind::Text {
        model: before,
        transform: old,
    }) = import(layer)
        && *model == before
        && transform == old
    {
        return Ok(());
    }
    let old = layer.info(b"TySh").map(|b| b.data.clone());
    let source = old
        .as_deref()
        .map(::psd::metadata::parse_text)
        .transpose()
        .map_err(error)?;
    let engine =
        typography::export_engine_data(model, source.as_ref().and_then(|s| s.engine_data()))
            .map_err(error)?;
    let mut descriptor = source
        .as_ref()
        .map(|s| s.descriptor.clone())
        .unwrap_or_else(empty);
    let mut warp = source
        .as_ref()
        .map(|s| s.warp.clone())
        .unwrap_or_else(empty);
    put(
        &mut descriptor,
        b"Txt ",
        V::Text(model.runs.iter().map(|r| r.text.as_str()).collect()),
    );
    put(&mut descriptor, b"EngineData", V::Raw(&engine));
    // EngineData owns the edited ranges. A stale descriptor range would override it.
    descriptor.items.retain(|(k, _)| *k != b"textStyleRange");
    put(
        &mut descriptor,
        b"Ornt",
        V::Enum {
            type_id: b"Ornt",
            value: if model.vertical { b"Vrtc" } else { b"Hrzn" },
        },
    );
    let kind = match model.warp.kind {
        typography::WarpKind::Arc => b"warpArc".as_slice(),
        typography::WarpKind::Flag => b"warpFlag",
        typography::WarpKind::Wave => b"warpWave",
    };
    put(
        &mut warp,
        b"warpStyle",
        V::Enum {
            type_id: b"warpStyle",
            value: if model.warp.amount == 0. {
                b"warpNone"
            } else {
                kind
            },
        },
    );
    put(
        &mut warp,
        b"warpValue",
        V::Double(f64::from(model.warp.amount) * 100.),
    );
    let [a, c, e, b, d, f] = transform.m;
    let mut out = 1u16.to_be_bytes().to_vec();
    for v in [a, b, c, d, e, f] {
        out.extend(v.to_be_bytes());
    }
    out.extend(50u16.to_be_bytes());
    out.extend(16u32.to_be_bytes());
    super::style_interop::descriptor(&descriptor, &mut out);
    out.extend(1u16.to_be_bytes());
    out.extend(16u32.to_be_bytes());
    super::style_interop::descriptor(&warp, &mut out);
    let bounds = match model.text_box {
        typography::TextBox::Paragraph { width, height } => {
            [0., 0., f64::from(width), f64::from(height)]
        }
        _ => source.as_ref().map(|s| s.bounds).unwrap_or([0.; 4]),
    };
    for v in bounds {
        out.extend(v.to_be_bytes());
    }
    let bridge = TextBridge {
        version: 1,
        model: model.clone(),
        fingerprint: *blake3::hash(&out).as_bytes(),
    };
    set_tag(layer, *b"TySh", out);
    set_tag(layer, *b"tvTx", serde_json::to_vec(&bridge).map_err(error)?);
    Ok(())
}

#[derive(serde::Serialize, serde::Deserialize)]
struct TextBridge {
    version: u32,
    model: typography::TextModel,
    fingerprint: [u8; 32],
}
fn restore_model(layer: &::psd::Layer) -> Option<typography::TextModel> {
    let tag = layer.info(b"tvTx")?;
    if tag.data.len() > 16 << 20 {
        return None;
    }
    let bridge: TextBridge = serde_json::from_slice(&tag.data).ok()?;
    if bridge.version != 1
        || bridge.fingerprint != *blake3::hash(&layer.info(b"TySh")?.data).as_bytes()
    {
        return None;
    }
    bridge.model.validate().ok()?;
    Some(bridge.model)
}
