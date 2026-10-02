use engine_api::{
    color::ColorMatrix3,
    recipe::{DevelopSettings, ProcessVersion, settings::WhiteBalanceMode},
};
use pipeline_cpu::{
    CameraLinearProxy, LensContext, ManualCaSettings, RenderSource, SmartPreviewEncoding,
    SmartPreviewTier, render_linear_scaled,
};
use raw_decode::{CfaImage, CfaLayout, RawMetadata};
fn fixture(w: u32, h: u32) -> (CfaImage, RawMetadata) {
    let m = RawMetadata {
        make: "synthetic".into(),
        model: "camera".into(),
        lens: None,
        iso: 100.,
        shutter_s: 0.01,
        aperture: 4.,
        focal_mm: 50.,
        capture_time: 0,
        catalog_orientation: None,
        baseline_exposure: 0.,
        orientation: 6,
        width: w,
        height: h,
        cfa_layout: CfaLayout::Bayer([[0, 1], [1, 2]]),
        black_levels: [0.; 4],
        white_level: 65535,
        as_shot_wb: [2., 1., 1.5, 1.],
        camera_to_xyz: ColorMatrix3::IDENTITY,
        cam_xyz: [[0.7, 0.2, 0.1], [0.1, 0.8, 0.1], [0.1, 0.2, 0.7], [0.; 3]],
        rgb_cam: [[0.; 4]; 3],
        default_crop: [0, 0, w, h],
        has_gain_map: false,
        has_opcode_list: false,
        opcode_lists: [None, None, None],
    };
    let c = CfaImage::from_linear(
        w,
        h,
        (0..w * h)
            .map(|i| 0.1 + (i % w) as f32 / (w as f32) * 0.15 + ((i / w) % 2) as f32 * 0.1)
            .collect(),
    )
    .unwrap();
    (c, m)
}

fn proxy() -> CameraLinearProxy {
    let (c, m) = fixture(32, 24);
    CameraLinearProxy::generate(
        &c,
        &m,
        &DevelopSettings::default(),
        ProcessVersion::NATIVE_CURRENT,
        [7; 32],
        &LensContext::default(),
    )
    .unwrap()
}
fn resign(bytes: &mut [u8]) {
    let mut h = blake3::Hasher::new();
    h.update(&bytes[..64]);
    h.update(&bytes[96..]);
    bytes[64..96].copy_from_slice(h.finalize().as_bytes());
}
fn change_json(bytes: &[u8], edit: impl FnOnce(&mut serde_json::Value)) -> Vec<u8> {
    let n = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
    let mut v: serde_json::Value = serde_json::from_slice(&bytes[96..96 + n]).unwrap();
    edit(&mut v);
    let json = serde_json::to_vec(&v).unwrap();
    let mut out = bytes[..96].to_vec();
    out[12..16].copy_from_slice(&(json.len() as u32).to_le_bytes());
    out.extend_from_slice(&json);
    out.extend_from_slice(&bytes[96 + n..]);
    resign(&mut out);
    out
}
fn snapshot(bytes: &[u8]) -> serde_json::Value {
    let n = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
    serde_json::from_slice(&bytes[96..96 + n]).unwrap()
}
#[test]
fn deterministic_round_trip_preserves_metadata_lens_prefix_and_editable_wb() {
    let (c, mut m) = fixture(32, 24);
    m.default_crop = [1, 1, 29, 21];
    let mut settings = DevelopSettings::default();
    let profile = lens::Profile {
        model: "nonidentity".into(),
        samples: vec![lens::CalibrationSample {
            vignette: [-0.2, 0.01, 0.],
            ca_red: [0.97, 0., 0.],
            distortion: lens::BrownConrady {
                k1: 0.012345678912345,
                ..Default::default()
            },
            ..Default::default()
        }],
        ..Default::default()
    };
    let context = LensContext {
        profile: Some(&profile),
        manual_ca: ManualCaSettings {
            red_cyan: 7.,
            blue_yellow: -4.,
        },
        ..Default::default()
    };
    let p = CameraLinearProxy::generate(
        &c,
        &m,
        &settings,
        ProcessVersion::NATIVE_CURRENT,
        [13; 32],
        &context,
    )
    .unwrap();
    let bytes = p.encode_persistent(123456).unwrap();
    assert_eq!(bytes, p.encode_persistent(123456).unwrap());
    let decoded = CameraLinearProxy::decode_persistent(&bytes).unwrap();
    assert_eq!(decoded.encoding, SmartPreviewEncoding::F16);
    assert_eq!(decoded.original_byte_length, 123456);
    assert_eq!(decoded.proxy.original_content_digest(), [13; 32]);
    assert_eq!(
        decoded.proxy.original_metadata().default_crop,
        m.default_crop
    );
    assert_eq!(decoded.proxy.original_metadata().orientation, 6);
    assert_eq!(
        snapshot(&bytes),
        snapshot(&decoded.proxy.encode_persistent(123456).unwrap())
    );
    for custom in [false, true] {
        if custom {
            settings.white_balance.mode = WhiteBalanceMode::Custom;
            settings.white_balance.temperature = 4200.;
            settings.white_balance.tint = 13.;
            settings.tone.exposure = 0.7;
        }
        let original = render_linear_scaled(&settings, &RenderSource::CameraLinear(&p), 1).unwrap();
        let reopened =
            render_linear_scaled(&settings, &RenderSource::CameraLinear(&decoded.proxy), 1)
                .unwrap();
        for (a, b) in original
            .planes()
            .iter()
            .flatten()
            .zip(reopened.planes().iter().flatten())
        {
            assert!((a - b).abs() <= 0.003 * a.abs() + 0.0005, "{a} != {b}");
        }
    }
    settings.lens.chromatic_aberration_scale = 12.;
    assert!(decoded.proxy.validate_prefix(&settings).is_err());
}
#[test]
fn malformed_header_lengths_versions_payload_digest_and_corruption_fail_closed() {
    let bytes = proxy().encode_persistent(100).unwrap();
    for n in [0, 1, 8, 95, bytes.len() - 1] {
        assert!(CameraLinearProxy::decode_persistent(&bytes[..n]).is_err());
    }
    for at in [0, 8, 12, 16, 24, 32, 64, 96, bytes.len() - 1] {
        let mut bad = bytes.clone();
        bad[at] ^= 1;
        assert!(
            CameraLinearProxy::decode_persistent(&bad).is_err(),
            "offset {at}"
        );
    }
    let mut bad = bytes.clone();
    bad[32] ^= 1;
    resign(&mut bad);
    assert!(CameraLinearProxy::decode_persistent(&bad).is_err());
    for range in [12..16, 16..24, 24..32] {
        let mut bad = bytes.clone();
        bad[range].fill(255);
        resign(&mut bad);
        assert!(CameraLinearProxy::decode_persistent(&bad).is_err());
    }
    let mut trailing = bytes.clone();
    trailing.push(0);
    assert!(CameraLinearProxy::decode_persistent(&trailing).is_err());
}
#[test]
fn authenticated_but_invalid_snapshot_is_rejected() {
    let bytes = proxy().encode_persistent(100).unwrap();
    for field in [
        "generator",
        "width",
        "height",
        "scale",
        "original_byte_length",
    ] {
        let bad = change_json(&bytes, |v| v[field] = serde_json::json!(0));
        assert!(
            CameraLinearProxy::decode_persistent(&bad).is_err(),
            "{field}"
        );
    }
    for (field, value) in [("width", 2561), ("scale", 2)] {
        let bad = change_json(&bytes, |v| v[field] = serde_json::json!(value));
        assert!(CameraLinearProxy::decode_persistent(&bad).is_err());
    }
    let bad = change_json(&bytes, |v| {
        v["metadata"]["default_crop"] = serde_json::json!([4294967295_u32, 0, 32, 24])
    });
    assert!(CameraLinearProxy::decode_persistent(&bad).is_err());
    let bad = change_json(&bytes, |v| {
        v["correction"]["source"] = serde_json::json!("Database");
        v["correction"]["sample"] = serde_json::Value::Null;
    });
    assert!(CameraLinearProxy::decode_persistent(&bad).is_err());
}
#[test]
fn bounded_decompression_rejects_compressed_bomb_with_small_declared_output() {
    let mut bytes = proxy().encode_persistent(100).unwrap();
    let n = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
    // Valid container hash, valid snapshot, but frame output greatly exceeds dimensions.
    let bomb = zstd::bulk::compress(&vec![0; 8 * 1024 * 1024], 3).unwrap();
    bytes.truncate(96 + n);
    bytes[16..24].copy_from_slice(&(bomb.len() as u64).to_le_bytes());
    bytes.extend_from_slice(&bomb);
    resign(&mut bytes);
    assert!(CameraLinearProxy::decode_persistent(&bytes).is_err());
}
#[test]
fn container_identity_binds_metadata_source_length_and_encoded_payload() {
    let p = proxy();
    let a = CameraLinearProxy::decode_persistent(&p.encode_persistent(100).unwrap()).unwrap();
    let b = CameraLinearProxy::decode_persistent(&p.encode_persistent(101).unwrap()).unwrap();
    assert_ne!(a.container_digest, b.container_digest);
    let bytes = p.encode_persistent(100).unwrap();
    let changed = change_json(&bytes, |v| {
        v["metadata"]["model"] = serde_json::json!("another camera")
    });
    let c = CameraLinearProxy::decode_persistent(&changed).unwrap();
    assert_ne!(a.container_digest, c.container_digest);
    // A valid container is not evidence that these source assertions match any photo.
    assert_eq!(
        a.proxy.original_content_digest(),
        c.proxy.original_content_digest()
    );
}

#[test]
fn odd_crop_scale_and_original_embedded_opcode_bytes_survive_reopen() {
    let (_, mut m) = fixture(2564, 6);
    m.default_crop = [1, 1, 2561, 3];
    let mut opcode: Vec<u8> = [1_u32, 3, 0x01030000, 0, 56]
        .into_iter()
        .flat_map(u32::to_be_bytes)
        .collect();
    for v in [0.2_f64, 0., 0., 0., 0., 0.5, 0.5] {
        opcode.extend(v.to_be_bytes());
    }
    m.opcode_lists[1] = Some(opcode);
    m.has_opcode_list = true;
    let c = CfaImage::from_linear(2564, 6, vec![2.; 2564 * 6]).unwrap();
    let settings = DevelopSettings::default();
    let p = CameraLinearProxy::generate(
        &c,
        &m,
        &settings,
        ProcessVersion::NATIVE_CURRENT,
        [9; 32],
        &Default::default(),
    )
    .unwrap();
    let d = CameraLinearProxy::decode_persistent(&p.encode_persistent(123).unwrap()).unwrap();
    assert_eq!(d.proxy.scale(), 2);
    assert_eq!(
        (d.proxy.pixels().width(), d.proxy.pixels().height()),
        (1281, 2)
    );
    assert_eq!(d.proxy.original_metadata().opcode_lists, m.opcode_lists);
    assert_eq!(d.proxy.original_metadata().default_crop, m.default_crop);
    assert_eq!(
        snapshot(&p.encode_persistent(123).unwrap())["correction"]["source"],
        "Embedded"
    );
    assert!(d.proxy.pixels().planes().iter().flatten().any(|v| *v > 1.));
    let a = render_linear_scaled(&settings, &RenderSource::CameraLinear(&p), 1).unwrap();
    let b = render_linear_scaled(&settings, &RenderSource::CameraLinear(&d.proxy), 1).unwrap();
    for (a, b) in a.planes().iter().flatten().zip(b.planes().iter().flatten()) {
        assert!((a - b).abs() < 0.003 * a.abs() + 0.0005);
    }
}

#[test]
fn authenticated_lens_mode_source_mismatches_are_rejected() {
    use engine_api::recipe::settings::{LensProfileRef, LensProfileSource};
    let bytes = proxy().encode_persistent(100).unwrap();
    let named = serde_json::to_value(LensProfileSource::Database {
        profile: LensProfileRef::named("required named profile"),
    })
    .unwrap();
    let sample = serde_json::to_value(lens::CalibrationSample {
        ca_red: [1.01, 0., 0.],
        ..Default::default()
    })
    .unwrap();
    for (mode, source, with_sample) in [
        (named.clone(), "Manual", false),
        (named, "Image", true),
        (serde_json::json!({"kind":"none"}), "Database", true),
        (
            serde_json::json!({"kind":"auto_calibrated"}),
            "Database",
            true,
        ),
    ] {
        let bad = change_json(&bytes, |v| {
            v["lens"]["profile"] = mode;
            v["correction"]["source"] = serde_json::json!(source);
            v["correction"]["sample"] = if with_sample {
                sample.clone()
            } else {
                serde_json::Value::Null
            };
        });
        let error = CameraLinearProxy::decode_persistent(&bad).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("inconsistent resolved lens snapshot"),
            "{error}"
        );
    }
    // None legitimately permits a captured image-based CA-only correction.
    let valid = change_json(&bytes, |v| {
        v["lens"]["profile"] = serde_json::json!({"kind":"none"});
        v["correction"]["source"] = serde_json::json!("Image");
        v["correction"]["sample"] = sample;
        v["lens"]["remove_chromatic_aberration"] = serde_json::json!(true);
    });
    assert!(CameraLinearProxy::decode_persistent(&valid).is_ok());
    let bad = change_json(&valid, |v| {
        v["lens"]["remove_chromatic_aberration"] = serde_json::json!(false)
    });
    assert!(CameraLinearProxy::decode_persistent(&bad).is_err());
}

#[test]
fn authenticated_missing_and_unknown_nested_members_fail_closed() {
    let bytes = change_json(&proxy().encode_persistent(100).unwrap(), |v| {
        v["correction"]["source"] = serde_json::json!("Image");
        v["correction"]["sample"] =
            serde_json::to_value(lens::CalibrationSample::default()).unwrap();
    });
    assert!(CameraLinearProxy::decode_persistent(&bytes).is_ok());
    for (path, member) in [
        ("/decode", "frame_index"),
        ("/linearize", "highlight_reconstruction"),
        ("/demosaic", "model"),
        ("/denoise", "amount"),
        ("/denoise/method", "kind"),
        ("/lens", "distortion_scale"),
        ("/lens/profile", "kind"),
        ("/lens/defringe_purple", "amount"),
        ("/lens/defringe_green", "hue_range"),
        ("/correction", "sample"),
        ("/correction/sample", "ca_red"),
        ("/correction/sample/distortion", "k1"),
    ] {
        for missing in [true, false] {
            let bad = change_json(&bytes, |v| {
                let object = v.pointer_mut(path).unwrap().as_object_mut().unwrap();
                if missing {
                    object.remove(member);
                } else {
                    object.insert("future_field".into(), serde_json::json!(1));
                }
            });
            assert!(
                CameraLinearProxy::decode_persistent(&bad).is_err(),
                "{path}/{member}, missing={missing}"
            );
        }
    }
    let named = change_json(&bytes, |v| {
        v["correction"]["source"] = serde_json::json!("Database");
        v["lens"]["profile"] =
            serde_json::to_value(engine_api::recipe::settings::LensProfileSource::Database {
                profile: engine_api::recipe::settings::LensProfileRef::named("profile"),
            })
            .unwrap();
    });
    assert!(CameraLinearProxy::decode_persistent(&named).is_ok());
    for missing in [true, false] {
        let bad = change_json(&named, |v| {
            let profile = v["lens"]["profile"]["profile"].as_object_mut().unwrap();
            if missing {
                profile.remove("filename");
            } else {
                profile.insert("future_field".into(), serde_json::json!(1));
            }
        });
        assert!(CameraLinearProxy::decode_persistent(&bad).is_err());
    }
}

#[test]
fn authenticated_duplicate_nested_member_is_rejected_before_value_validation() {
    let bytes = proxy().encode_persistent(100).unwrap();
    let n = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
    let json = std::str::from_utf8(&bytes[96..96 + n]).unwrap();
    let duplicate = json.replace("\"frame_index\":0", "\"frame_index\":0,\"frame_index\":0");
    assert_ne!(duplicate, json);
    let mut bad = bytes[..96].to_vec();
    bad[12..16].copy_from_slice(&(duplicate.len() as u32).to_le_bytes());
    bad.extend_from_slice(duplicate.as_bytes());
    bad.extend_from_slice(&bytes[96 + n..]);
    resign(&mut bad);
    assert!(
        CameraLinearProxy::decode_persistent(&bad)
            .unwrap_err()
            .to_string()
            .contains("duplicate field")
    );
}

/// Opt-in read-only fixture qualification. All generated artifacts use the explicit output directory.
#[test]
#[ignore = "requires TESSERA_CODEC_RAW_FIXTURE and TESSERA_CODEC_OUTPUT_DIR"]
fn real_raw_fixture_measurement() {
    use std::time::Instant;
    let path = std::env::var("TESSERA_CODEC_RAW_FIXTURE").unwrap();
    let output = std::path::PathBuf::from(std::env::var("TESSERA_CODEC_OUTPUT_DIR").unwrap());
    let tier = match std::env::var("TESSERA_CODEC_TIER").as_deref() {
        Ok("compact2048") => SmartPreviewTier::Compact2048,
        Ok("detail2560") | Err(std::env::VarError::NotPresent) => SmartPreviewTier::Detail2560,
        _ => panic!("TESSERA_CODEC_TIER must be compact2048 or detail2560"),
    };
    let source_bytes = std::fs::read(&path).unwrap();
    let source_digest = *blake3::hash(&source_bytes).as_bytes();
    let mut source = raw_decode::RawSource::open(&path).unwrap();
    let start = Instant::now();
    let cfa = source.decode_cfa().unwrap();
    let metadata = source.metadata();
    let raw_decode_ms = start.elapsed().as_secs_f64() * 1000.;
    let settings = DevelopSettings::default();
    let start = Instant::now();
    let proxy = CameraLinearProxy::generate_with_tier(
        &cfa,
        &metadata,
        &settings,
        ProcessVersion::NATIVE_CURRENT,
        source_digest,
        &LensContext::default(),
        tier,
    )
    .unwrap();
    let generation_ms = start.elapsed().as_secs_f64() * 1000.;
    let start = Instant::now();
    let encoded = proxy.encode_persistent(source_bytes.len() as u64).unwrap();
    let encode_ms = start.elapsed().as_secs_f64() * 1000.;
    let start = Instant::now();
    let reopened = CameraLinearProxy::decode_persistent(&encoded).unwrap();
    let decode_ms = start.elapsed().as_secs_f64() * 1000.;
    let mut max_sample_error = 0.0_f64;
    for (a, b) in proxy
        .pixels()
        .planes()
        .iter()
        .flatten()
        .zip(reopened.proxy.pixels().planes().iter().flatten())
    {
        let error = (f64::from(*a) - f64::from(*b)).abs();
        max_sample_error = max_sample_error.max(error);
        assert!(error <= 0.0005 * f64::from(*a).abs() + 3e-8);
    }
    let start = Instant::now();
    let baseline =
        render_linear_scaled(&settings, &RenderSource::CameraLinear(&reopened.proxy), 1).unwrap();
    let baseline_render_ms = start.elapsed().as_secs_f64() * 1000.;
    let mut edited = settings.clone();
    edited.white_balance.mode = WhiteBalanceMode::Custom;
    edited.white_balance.temperature = 4200.;
    edited.white_balance.tint = 13.;
    edited.tone.exposure = 0.7;
    let start = Instant::now();
    let rendered =
        render_linear_scaled(&edited, &RenderSource::CameraLinear(&reopened.proxy), 1).unwrap();
    let edited_render_ms = start.elapsed().as_secs_f64() * 1000.;
    let unencoded = render_linear_scaled(&edited, &RenderSource::CameraLinear(&proxy), 1).unwrap();
    let mut max_edited_render_error = 0.0_f64;
    for (a, b) in unencoded
        .planes()
        .iter()
        .flatten()
        .zip(rendered.planes().iter().flatten())
    {
        let error = (f64::from(*a) - f64::from(*b)).abs();
        max_edited_render_error = max_edited_render_error.max(error);
        assert!(error <= 0.003 * f64::from(*a).abs() + 0.0005);
    }
    assert_ne!(baseline.planes(), rendered.planes());
    assert!(rendered.planes().iter().flatten().all(|v| v.is_finite()));
    let mut render_hash = blake3::Hasher::new();
    for sample in rendered.planes().iter().flatten() {
        render_hash.update(&sample.to_le_bytes());
    }
    let report = serde_json::json!({
        "original_bytes": source_bytes.len(), "original_blake3": blake3::Hash::from(source_digest).to_hex().to_string(),
        "sensor_dimensions": [metadata.width, metadata.height], "original_crop": metadata.default_crop,
        "tier": format!("{tier:?}"), "max_edge": tier.max_edge(),
        "proxy_dimensions": [proxy.pixels().width(), proxy.pixels().height()], "scale": proxy.scale(),
        "proxy_encoded_bytes": encoded.len(), "encoding": format!("{:?}", reopened.encoding),
        "raw_decode_ms": raw_decode_ms, "generation_ms": generation_ms, "encode_ms": encode_ms, "decode_ms": decode_ms,
        "max_sample_error": max_sample_error, "max_edited_render_error":max_edited_render_error, "baseline_render_ms": baseline_render_ms, "edited_render_ms": edited_render_ms,
        "edit": { "temperature": 4200, "tint": 13, "exposure": 0.7 }, "edit_changed_render": true,
        "edited_render_dimensions": [rendered.width(), rendered.height()],
        "edited_render_blake3": render_hash.finalize().to_hex().to_string()
    });
    assert_eq!(
        *blake3::hash(&std::fs::read(&path).unwrap()).as_bytes(),
        source_digest
    );
    let (asset_name, report_name) = match tier {
        SmartPreviewTier::Detail2560 => ("sony-camera-linear.clp", "sony-measurement.json"),
        SmartPreviewTier::Compact2048 => (
            "sony-compact2048-camera-linear.clp",
            "sony-compact2048-measurement.json",
        ),
    };
    std::fs::write(output.join(asset_name), encoded).unwrap();
    std::fs::write(
        output.join(report_name),
        serde_json::to_vec_pretty(&report).unwrap(),
    )
    .unwrap();
    println!("{report}");
}

#[test]
fn explicit_tiers_round_trip_with_validated_scale_and_unchanged_default() {
    let (cfa, metadata) = fixture(4920, 9);
    let settings = DevelopSettings::default();
    let default = CameraLinearProxy::generate(
        &cfa,
        &metadata,
        &settings,
        ProcessVersion::NATIVE_CURRENT,
        [7; 32],
        &LensContext::default(),
    )
    .unwrap();
    for (tier, scale, width, height) in [
        (SmartPreviewTier::Detail2560, 2, 2460, 5),
        (SmartPreviewTier::Compact2048, 3, 1640, 3),
    ] {
        let proxy = CameraLinearProxy::generate_with_tier(
            &cfa,
            &metadata,
            &settings,
            ProcessVersion::NATIVE_CURRENT,
            [7; 32],
            &LensContext::default(),
            tier,
        )
        .unwrap();
        assert_eq!(
            (
                proxy.scale(),
                proxy.pixels().width(),
                proxy.pixels().height()
            ),
            (scale, width, height)
        );
        if tier == SmartPreviewTier::Detail2560 {
            assert_eq!(default.pixels().planes(), proxy.pixels().planes());
            assert_eq!(default.tier(), tier);
        }
        let bytes = proxy.encode_persistent(123).unwrap();
        let decoded = CameraLinearProxy::decode_persistent(&bytes).unwrap();
        assert_eq!(decoded.proxy.tier(), tier);
        assert_eq!(decoded.proxy.scale(), scale);
        assert_eq!(
            decoded.proxy.original_metadata().default_crop,
            metadata.default_crop
        );
        for (a, b) in proxy
            .pixels()
            .planes()
            .iter()
            .flatten()
            .zip(decoded.proxy.pixels().planes().iter().flatten())
        {
            assert!((f64::from(*a) - f64::from(*b)).abs() <= 0.0005 * f64::from(*a).abs() + 3e-8);
        }
        let mut edited = settings.clone();
        edited.white_balance.mode = WhiteBalanceMode::Custom;
        edited.white_balance.temperature = 4200.;
        edited.white_balance.tint = 13.;
        edited.tone.exposure = 0.7;
        let before = render_linear_scaled(&edited, &RenderSource::CameraLinear(&proxy), 1).unwrap();
        let after =
            render_linear_scaled(&edited, &RenderSource::CameraLinear(&decoded.proxy), 1).unwrap();
        for (a, b) in before
            .planes()
            .iter()
            .flatten()
            .zip(after.planes().iter().flatten())
        {
            assert!((a - b).abs() <= 0.003 * a.abs() + 0.0005);
        }
        let mismatched = change_json(&bytes, |v| {
            v["tier"] = serde_json::json!(if tier == SmartPreviewTier::Detail2560 {
                "Compact2048"
            } else {
                "Detail2560"
            })
        });
        assert!(CameraLinearProxy::decode_persistent(&mismatched).is_err());
    }
}

#[test]
fn legacy_v1_detail_decodes_but_new_container_requires_explicit_supported_tier() {
    let bytes = proxy().encode_persistent(100).unwrap();
    assert_eq!(u32::from_le_bytes(bytes[8..12].try_into().unwrap()), 2);
    let mut legacy = change_json(&bytes, |v| {
        v.as_object_mut().unwrap().remove("tier");
        v["generator"] = serde_json::json!(1);
    });
    legacy[8..12].copy_from_slice(&1_u32.to_le_bytes());
    resign(&mut legacy);
    let decoded = CameraLinearProxy::decode_persistent(&legacy).unwrap();
    assert_eq!(decoded.proxy.tier(), SmartPreviewTier::Detail2560);
    assert_eq!(decoded.proxy.scale(), 1);
    assert_eq!(
        decoded.proxy.pixels().planes(),
        CameraLinearProxy::decode_persistent(&bytes)
            .unwrap()
            .proxy
            .pixels()
            .planes()
    );
    let upgraded = decoded.proxy.encode_persistent(100).unwrap();
    assert_eq!(u32::from_le_bytes(upgraded[8..12].try_into().unwrap()), 2);
    assert!(CameraLinearProxy::decode_persistent(&upgraded).is_ok());
    for tier in [serde_json::Value::Null, serde_json::json!("Future4096")] {
        assert!(
            CameraLinearProxy::decode_persistent(&change_json(&bytes, |v| v["tier"] = tier))
                .is_err()
        );
    }
    assert!(
        CameraLinearProxy::decode_persistent(&change_json(&bytes, |v| {
            v.as_object_mut().unwrap().remove("tier");
        }))
        .is_err()
    );
    assert!(
        CameraLinearProxy::decode_persistent(
            &change_json(&legacy, |v| v["tier"] = serde_json::json!("Compact2048"))
        )
        .is_err()
    );
    assert!(
        CameraLinearProxy::decode_persistent(
            &change_json(&bytes, |v| v["generator"] = serde_json::json!(1))
        )
        .is_err()
    );
}

#[test]
fn legacy_v1_rejects_even_null_tier_metadata() {
    let bytes = proxy().encode_persistent(100).unwrap();
    let mut legacy = change_json(&bytes, |v| {
        v["generator"] = serde_json::json!(1);
        v["tier"] = serde_json::Value::Null;
    });
    legacy[8..12].copy_from_slice(&1_u32.to_le_bytes());
    resign(&mut legacy);
    assert!(CameraLinearProxy::decode_persistent(&legacy).is_err());
}

/// Reopen an actual preserved v1 asset, not reconstructed metadata from a v2 writer.
#[test]
#[ignore = "requires TESSERA_CODEC_LEGACY_ASSET, TESSERA_CODEC_LEGACY_REPORT and TESSERA_CODEC_OUTPUT_DIR"]
fn real_legacy_v1_asset_reopen_and_edit_parity() {
    use std::time::Instant;
    let path = std::env::var("TESSERA_CODEC_LEGACY_ASSET").unwrap();
    let report_path = std::env::var("TESSERA_CODEC_LEGACY_REPORT").unwrap();
    let output = std::path::PathBuf::from(std::env::var("TESSERA_CODEC_OUTPUT_DIR").unwrap());
    let bytes = std::fs::read(&path).unwrap();
    let asset_hash = blake3::hash(&bytes);
    assert_eq!(u32::from_le_bytes(bytes[8..12].try_into().unwrap()), 1);
    let historical: serde_json::Value =
        serde_json::from_slice(&std::fs::read(report_path).unwrap()).unwrap();
    let start = Instant::now();
    let legacy = CameraLinearProxy::decode_persistent(&bytes).unwrap();
    let decode_ms = start.elapsed().as_secs_f64() * 1000.;
    assert_eq!(legacy.proxy.tier(), SmartPreviewTier::Detail2560);
    assert_eq!(
        (
            legacy.proxy.pixels().width(),
            legacy.proxy.pixels().height()
        ),
        (2460, 1638)
    );
    assert_eq!(
        legacy.original_byte_length,
        historical["original_bytes"].as_u64().unwrap()
    );
    assert_eq!(
        blake3::Hash::from(legacy.proxy.original_content_digest())
            .to_hex()
            .as_str(),
        historical["original_blake3"].as_str().unwrap()
    );
    let upgraded_bytes = legacy
        .proxy
        .encode_persistent(legacy.original_byte_length)
        .unwrap();
    let upgraded = CameraLinearProxy::decode_persistent(&upgraded_bytes).unwrap();
    assert_eq!(
        legacy.proxy.pixels().planes(),
        upgraded.proxy.pixels().planes()
    );
    let settings = DevelopSettings::default();
    let baseline =
        render_linear_scaled(&settings, &RenderSource::CameraLinear(&legacy.proxy), 1).unwrap();
    let mut edited = settings;
    edited.white_balance.mode = WhiteBalanceMode::Custom;
    edited.white_balance.temperature = 4200.;
    edited.white_balance.tint = 13.;
    edited.tone.exposure = 0.7;
    let start = Instant::now();
    let rendered =
        render_linear_scaled(&edited, &RenderSource::CameraLinear(&legacy.proxy), 1).unwrap();
    let edited_render_ms = start.elapsed().as_secs_f64() * 1000.;
    let upgraded_render =
        render_linear_scaled(&edited, &RenderSource::CameraLinear(&upgraded.proxy), 1).unwrap();
    assert_eq!(rendered.planes(), upgraded_render.planes());
    assert_ne!(baseline.planes(), rendered.planes());
    let mut hash = blake3::Hasher::new();
    for value in rendered.planes().iter().flatten() {
        hash.update(&value.to_le_bytes());
    }
    let render_hash = hash.finalize().to_hex().to_string();
    assert_eq!(
        render_hash,
        historical["edited_render_blake3"].as_str().unwrap()
    );
    assert_eq!(blake3::hash(&std::fs::read(&path).unwrap()), asset_hash);
    let report = serde_json::json!({"legacy_bytes":bytes.len(),"legacy_blake3":asset_hash.to_hex().to_string(),"legacy_version":1,"decoded_tier":"Detail2560","dimensions":[2460,1638],"decode_ms":decode_ms,"edited_render_ms":edited_render_ms,"edited_render_blake3":render_hash,"matches_historical_render":true,"v2_upgrade_samples_exact":true,"v2_upgrade_edit_exact":true,"legacy_file_unchanged":true});
    std::fs::write(
        output.join("legacy-v1-reopen.json"),
        serde_json::to_vec_pretty(&report).unwrap(),
    )
    .unwrap();
    println!("{report}");
}

#[test]
fn legacy_ca_lr7b_optional_prefix_survives_persistent_codec() {
    let (c, m) = fixture(32, 24);
    let mut settings = DevelopSettings::default();
    settings.lens.legacy_ca_red = Some(35.);
    settings.lens.legacy_ca_blue = Some(-25.);
    let p = CameraLinearProxy::generate(
        &c,
        &m,
        &settings,
        ProcessVersion::NATIVE_CURRENT,
        [7; 32],
        &LensContext::default(),
    )
    .unwrap();
    let bytes = p.encode_persistent(12345).unwrap();
    let decoded = CameraLinearProxy::decode_persistent(&bytes).unwrap();
    assert_eq!(bytes, decoded.proxy.encode_persistent(12345).unwrap());
    assert!(
        render_linear_scaled(&settings, &RenderSource::CameraLinear(&decoded.proxy), 1).is_ok()
    );
    settings.lens.legacy_ca_red = Some(36.);
    assert!(
        render_linear_scaled(&settings, &RenderSource::CameraLinear(&decoded.proxy), 1).is_err()
    );
}
