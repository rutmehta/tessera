use compositor::psd::{from_psd, to_psd};
use psd::PsdDocument;

// Byte-level fixture, not emitted by our PSD writer. Flattened RGB with a
// fourth saved selection, no negative layer count / merged transparency.
fn external_flattened() -> PsdDocument {
    let names = [0, 0, 0, 2, 0, b'A', 3, 0xbb]; // Aλ
    let mut resource = b"8BIM".to_vec();
    resource.extend_from_slice(&1045u16.to_be_bytes());
    resource.extend_from_slice(&[0, 0]);
    resource.extend_from_slice(&(names.len() as u32).to_be_bytes());
    resource.extend_from_slice(&names);
    let bytes = [
        b"8BPS".as_slice(),
        &1u16.to_be_bytes(),
        &[0; 6],
        &4u16.to_be_bytes(),
        &1u32.to_be_bytes(),
        &2u32.to_be_bytes(),
        &8u16.to_be_bytes(),
        &3u16.to_be_bytes(),
        &[0; 4],
        &(resource.len() as u32).to_be_bytes(),
        &resource,
        &[0; 4],
        &[0, 0],
        &[255, 0, 0, 255, 0, 0, 64, 192],
    ]
    .concat();
    PsdDocument::read(&bytes).unwrap()
}

#[test]
fn named_plane_becomes_editable_channel_and_export_refreshes_edits() {
    let mut imported = from_psd(&external_flattened()).unwrap();
    assert_eq!(imported.channels.len(), 1);
    assert_eq!(imported.channels[0].name, "Aλ");
    assert_eq!(imported.channels[0].raster.channels(), 1);
    assert_eq!(imported.channels[0].raster.pixel(0, 0)[0], 64.0 / 255.0);
    imported.channels[0].name = "edited 🦀".into();
    imported.channels[0]
        .raster
        .edit_region(compositor::Rect::new(0, 0, 2, 1), 1, |_, _, p| p[0] = 1.0)
        .unwrap();
    let exported = to_psd(&imported).unwrap();
    assert_eq!(&exported.composite[6..], &[255, 255]);
    assert_eq!(exported.alpha_names().unwrap().unwrap(), ["edited 🦀"]);
    let again = from_psd(&PsdDocument::read(&exported.write().unwrap()).unwrap()).unwrap();
    assert_eq!(again.channels[0].raster.pixel(0, 0)[0], 1.0);
}

#[test]
fn external_spot_display_info_maps_color_solidity_and_preserves_opaque_resources() {
    use compositor::channels::ChannelKind;
    for modern in [false, true] {
        let mut source = external_flattened();
        let mut data = if modern { vec![0, 0, 0, 1] } else { vec![] };
        data.extend_from_slice(&[0, 0, 255, 255, 0, 0, 0, 0, 0, 0, 0, 75, 2]);
        if !modern {
            data.push(0);
        }
        source.resources.push(psd::ImageResource::new(
            if modern { 1077 } else { 1007 },
            data,
        ));
        let opaque = psd::ImageResource::new(4000, vec![9, 8, 7]);
        source.resources.push(opaque.clone());
        let mut imported = from_psd(&source).unwrap();
        assert_eq!(
            imported.channels[0].kind,
            ChannelKind::Spot {
                color: [1.0, 0.0, 0.0],
                solidity: 0.75
            }
        );
        imported.channels[0].kind = ChannelKind::Spot {
            color: [0.0, 1.0, 0.0],
            solidity: 0.25,
        };
        let exported = to_psd(&imported).unwrap();
        assert_eq!(
            &exported.composite[..6],
            &source.composite[..6],
            "spot placeholder must not tint RGB"
        );
        assert!(exported.resources.contains(&opaque));
        let display = exported.channel_display_info().unwrap().unwrap();
        assert_eq!(display[0].color, [0, 65535, 0, 0]);
        assert_eq!(display[0].opacity, 25);
        assert_eq!(display[0].mode, 2);
        let again = from_psd(&PsdDocument::read(&exported.write().unwrap()).unwrap()).unwrap();
        assert_eq!(again.channels[0].kind, imported.channels[0].kind);
    }
}

#[test]
fn native_channels_roundtrip_all_depths_versions_and_compressions() {
    use compositor::{
        Depth, DocState, Raster,
        channels::{ChannelId, ChannelKind, DocumentChannel},
    };
    let extent = engine_api::tile::Extent::new(2, 1);
    for depth in [Depth::U8, Depth::U16, Depth::F32] {
        for channel_depth in [Depth::U8, Depth::U16, Depth::F32] {
            let mut state = DocState::new(extent, depth);
            // An otherwise empty canvas still needs explicit merged transparency.
            state.channels = vec![
                DocumentChannel {
                    id: ChannelId(1),
                    name: "selection λ".into(),
                    kind: ChannelKind::Alpha,
                    raster: Raster::new(extent, 1, channel_depth, 0.25),
                },
                DocumentChannel {
                    id: ChannelId(2),
                    name: "Ink".into(),
                    kind: ChannelKind::Spot {
                        color: [0.0, 0.0, 1.0],
                        solidity: 0.6,
                    },
                    raster: Raster::new(extent, 1, channel_depth, 0.75),
                },
            ];
            state.next_channel_id = 3;
            let imported = compositor::psd::ImportedPsd::from_state(state).unwrap();
            let exported = to_psd(&imported).unwrap();
            assert_eq!(exported.channels, 6);
            assert!(exported.layer_section.merged_alpha);
            for version in [psd::Version::Psd, psd::Version::Psb] {
                for compression in [
                    psd::Compression::Raw,
                    psd::Compression::Rle,
                    psd::Compression::Zip,
                ] {
                    let mut out = exported.clone();
                    out.version = version;
                    out.compression = compression;
                    let again =
                        from_psd(&PsdDocument::read(&out.write().unwrap()).unwrap()).unwrap();
                    assert_eq!(again.channels.len(), 2);
                    assert_eq!(again.channels[0].name, "selection λ");
                    assert_eq!(again.channels[1].kind, imported.channels[1].kind);
                    assert_eq!(again.next_channel_id, 3);
                    for i in 0..2 {
                        let expected = imported.channels[i].raster.pixel(0, 0)[0];
                        assert!(
                            (again.channels[i].raster.pixel(0, 0)[0] - expected).abs()
                                <= 1.0 / 255.0
                        );
                    }
                    let rendered = compositor::Compositor::new(1 << 20)
                        .render_level_rgba(&compositor::Document::new(again.state), 0)
                        .unwrap()
                        .1;
                    assert!(
                        rendered.iter().all(|v| *v == 0.0),
                        "spot channels must not paint an empty canvas"
                    );
                }
            }
        }
    }
}

#[test]
fn deleting_channels_removes_stale_planes_and_owned_metadata() {
    let mut imported = from_psd(&external_flattened()).unwrap();
    imported.channels.clear();
    let out = to_psd(&imported).unwrap();
    assert_eq!(out.channels, 3);
    assert_eq!(out.composite.len(), 6);
    assert!(out.alpha_names().unwrap().is_none());
    assert!(out.channel_display_info().unwrap().is_none());
}

#[test]
fn merged_transparency_and_saved_plane_use_distinct_offsets() {
    for metadata_includes_transparency in [false, true] {
        let mut source = external_flattened();
        source.channels = 5;
        source.layer_section.merged_alpha = true;
        source.composite.splice(6..6, [0, 128]);
        if metadata_includes_transparency {
            source.resources[0].data.splice(0..0, [0, 0, 0, 1, 0, b'T']);
        }
        let imported = from_psd(&source).unwrap();
        assert_eq!(imported.channels.len(), 1);
        assert_eq!(imported.channels[0].name, "Aλ");
        assert_eq!(imported.channels[0].raster.pixel(0, 0)[0], 64.0 / 255.0);
        assert_eq!(imported.root[0].raster().unwrap().pixel(0, 0)[3], 0.0);
        assert_eq!(&to_psd(&imported).unwrap().composite[8..], &[64, 192]);
    }
}

#[test]
fn adding_layer_transparency_does_not_overwrite_saved_alpha() {
    let mut imported = from_psd(&external_flattened()).unwrap();
    std::sync::Arc::make_mut(&mut imported.root[0])
        .raster_mut()
        .unwrap()
        .edit_region(compositor::Rect::new(0, 0, 2, 1), 1, |_, _, p| p[3] = 0.5)
        .unwrap();
    let out = to_psd(&imported).unwrap();
    assert!(out.layer_section.merged_alpha);
    assert_eq!(out.channels, 5);
    assert_eq!(&out.composite[6..8], &[128, 128]);
    assert_eq!(&out.composite[8..], &[64, 192]);
}

#[test]
fn invalid_plane_counts_metadata_counts_and_shapes_are_rejected() {
    let mut source = external_flattened();
    source.composite.pop();
    assert!(from_psd(&source).is_err());
    source = external_flattened();
    source.resources[0].data.extend_from_slice(&[0, 0, 0, 0]);
    assert!(from_psd(&source).is_err());
    source = external_flattened();
    source.channels = 3;
    source.layer_section.merged_alpha = true;
    source.composite.truncate(6);
    assert!(from_psd(&source).is_err());
    let mut imported = from_psd(&external_flattened()).unwrap();
    imported.channels[0].raster = compositor::Raster::new(imported.canvas, 4, imported.depth, 0.0);
    assert!(to_psd(&imported).is_err());
    imported.channels[0].raster =
        compositor::Raster::new(engine_api::tile::Extent::new(1, 1), 1, imported.depth, 0.0);
    assert!(to_psd(&imported).is_err());
}

#[test]
fn malformed_display_info_is_rejected() {
    let mut source = external_flattened();
    source
        .resources
        .push(psd::ImageResource::new(1077, vec![0, 0, 0, 1, 0]));
    assert!(from_psd(&source).is_err());
}

#[test]
fn malformed_names_are_rejected() {
    let mut source = external_flattened();
    source.resources[0].data.pop();
    assert!(from_psd(&source).is_err());
}

#[test]
fn fourth_named_plane_is_not_flattened_transparency() {
    let imported = from_psd(&external_flattened()).unwrap();
    assert_eq!(imported.root[0].raster().unwrap().pixel(0, 0)[3], 1.0);
    // Export must not replace saved selection samples with merged opacity.
    assert_eq!(&to_psd(&imported).unwrap().composite[6..], &[64, 192]);
}
