use super::*;
use engine_api::color::ColorMatrix3;
use engine_api::id::{Digest, ImageId};
use engine_api::pinned_raw::{PinnedRawDecoderRoute, PinnedRawInput};
use engine_api::recipe::settings::{DemosaicMethod, LensProfileSource};
use raw_decode::capture::CapturedAssetIdentity;
use raw_decode::{CfaLayout, CfaU16, RawMetadata};

const NARROW: &str = r#"{"demosaic":{"method":"bilinear"},"lens":{"profile":{"kind":"none"},"remove_chromatic_aberration":false}}"#;

fn descriptor(settings: &str) -> PinnedRawDescriptor {
    let json = format!(
        r#"{{"schema_version":3,"image_id":"{}","source_kind":"raw","process_version":{{"family":"native","revision":2}},"settings":{settings}}}"#,
        ImageId(1)
    );
    PinnedRawDescriptor::new(PinnedRawInput {
        asset_digest: Digest::derive("tessera pinned RAW asset v1", b"synthetic"),
        asset_byte_len: 9,
        recipe_image_id: ImageId(1),
        recipe_json: json.into_bytes(),
        decoder_route: PinnedRawDecoderRoute::LibRawCfaV1,
        suffix_hint: "arw".into(),
        locator_hint: None,
    })
    .expect("fixture must pass the existing descriptor authority")
}

fn narrow_recipe() -> Recipe {
    let mut recipe = Recipe::new(ImageId(1));
    recipe.settings.demosaic.method = DemosaicMethod::Bilinear;
    recipe.settings.lens.profile = LensProfileSource::None;
    recipe.settings.lens.remove_chromatic_aberration = false;
    recipe
}

fn from_recipe(recipe: &Recipe) -> PinnedRawDescriptor {
    PinnedRawDescriptor::new(PinnedRawInput {
        asset_digest: Digest::derive("tessera pinned RAW asset v1", b"synthetic"),
        asset_byte_len: 9,
        recipe_image_id: ImageId(1),
        recipe_json: recipe.to_json().unwrap(),
        decoder_route: PinnedRawDecoderRoute::LibRawCfaV1,
        suffix_hint: "arw".into(),
        locator_hint: None,
    })
    .unwrap()
}

fn decoded() -> DecodedCapturedCfa {
    let layout = CfaLayout::Bayer([[0, 1], [3, 2]]);
    DecodedCapturedCfa {
        image: CfaU16 {
            width: 6,
            height: 4,
            data: vec![1000; 24],
            cfa_layout: layout,
        },
        metadata: RawMetadata {
            make: "Synthetic".into(),
            model: "Admission only".into(),
            lens: None,
            iso: 100.0,
            shutter_s: 0.01,
            aperture: 4.0,
            focal_mm: 50.0,
            capture_time: 0,
            catalog_orientation: None,
            baseline_exposure: 0.,
            orientation: 1,
            width: 6,
            height: 4,
            cfa_layout: layout,
            black_levels: [64.0; 4],
            white_level: 4095,
            as_shot_wb: [2.0, 1.0, 1.5, 1.0],
            camera_to_xyz: ColorMatrix3([[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]),
            cam_xyz: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0], [0.0; 3]],
            rgb_cam: [
                [1.0, 0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
            ],
            default_crop: [1, 1, 3, 2],
            has_gain_map: false,
            has_opcode_list: false,
            opcode_lists: [None, None, None],
        },
        identity: CapturedAssetIdentity {
            digest: Digest::derive("synthetic only", b"not authenticated"),
            byte_len: 9,
        },
        route: PinnedRawDecoderRoute::LibRawCfaV1,
    }
}

// Break caught: reserializing settings to require all omitted defaults, or dropping detail/tone.
#[test]
fn omitted_defaults_and_supported_detail_are_preserved() {
    let d = descriptor(NARROW);
    let before = d.recipe_json().to_vec();
    let recipe = admit_recipe(&d).unwrap();
    assert_eq!(recipe.settings, narrow_recipe().settings);
    assert_eq!(d.recipe_json(), before);
    let mut detailed = narrow_recipe();
    detailed.settings.tone.exposure = 0.75;
    detailed.settings.detail.sharpening.amount = 62.0;
    let d = from_recipe(&detailed);
    assert_eq!(admit_recipe(&d).unwrap().settings, detailed.settings);
    assert_eq!(d.recipe_json(), detailed.to_json().unwrap());
}

// Break caught: rejecting descriptor-compatible empty legacy reference as an invalid shape.
#[test]
fn legacy_empty_camera_reference_keeps_default_compatibility() {
    let settings =
        NARROW.strip_suffix('}').unwrap().to_owned() + r#", "camera_profile":{"profile":""}}"#;
    let d = descriptor(&settings);
    assert_eq!(admit_recipe(&d).unwrap().settings, narrow_recipe().settings);
    assert!(
        std::str::from_utf8(d.recipe_json())
            .unwrap()
            .contains(r#""profile":"""#)
    );
}

// Break caught: treating declared Native2 or CPU validation alone as dependency admission.
#[test]
fn common_default_auto_demosaic_and_optics_are_not_sanitized() {
    for (settings, expected) in [
        (
            r#"{"lens":{"profile":{"kind":"none"},"remove_chromatic_aberration":false}}"#,
            Refusal::Demosaic,
        ),
        (r#"{"demosaic":{"method":"bilinear"}}"#, Refusal::Lens),
    ] {
        let d = descriptor(settings);
        let before = d.recipe_json().to_vec();
        assert_eq!(admit_recipe(&d).unwrap_err(), expected);
        assert_eq!(d.recipe_json(), before);
    }
}

#[test]
fn explicit_camera_and_lens_profiles_refuse_after_descriptor_compatibility() {
    let camera = NARROW.strip_suffix('}').unwrap().to_owned()
        + r#", "camera_profile":{"profile":"Legacy camera"}}"#;
    assert_eq!(
        admit_recipe(&descriptor(&camera)).unwrap_err(),
        Refusal::CameraProfile
    );
    let lens = r#"{"demosaic":{"method":"bilinear"},"lens":{"profile":{"kind":"database","profile":"Legacy lens"},"remove_chromatic_aberration":false}}"#;
    assert_eq!(admit_recipe(&descriptor(lens)).unwrap_err(), Refusal::Lens);
}

#[test]
fn changed_manual_lens_and_camera_profile_settings_refuse() {
    let mut recipe = narrow_recipe();
    recipe.settings.lens.manual_distortion = 0.1;
    assert_eq!(
        admit_recipe(&from_recipe(&recipe)).unwrap_err(),
        Refusal::Lens
    );
    let mut recipe = narrow_recipe();
    recipe.settings.camera_profile.amount = 99.0;
    assert_eq!(
        admit_recipe(&from_recipe(&recipe)).unwrap_err(),
        Refusal::CameraProfile
    );
}

// Break caught: accepting unknown settings outside descriptor::check_shape.
#[test]
fn descriptor_authority_rejects_unknown_fields_before_profile_admission() {
    let mut input = PinnedRawInput {
        asset_digest: Digest::default(),
        asset_byte_len: 1,
        recipe_image_id: ImageId(1),
        recipe_json: Vec::new(),
        decoder_route: PinnedRawDecoderRoute::LibRawCfaV1,
        suffix_hint: "arw".into(),
        locator_hint: None,
    };
    let d = descriptor(NARROW);
    let json = std::str::from_utf8(d.recipe_json()).unwrap();
    input.recipe_json = json
        .replace(
            r#""method":"bilinear""#,
            r#""method":"bilinear","future":true"#,
        )
        .into_bytes();
    assert_ne!(input.recipe_json, d.recipe_json());
    assert!(PinnedRawDescriptor::new(input).is_err());
}

// Break caught: accepting a caller's public route declaration as actual source classification.
#[test]
fn decoded_bayer_metadata_facts_are_not_source_authentication() {
    let a = decoded();
    assert_eq!(
        admit_metadata(&a).unwrap(),
        MetadataFacts {
            sensor_pixels: 24,
            active_pixels: 6
        }
    );
    let mut b = decoded();
    b.identity.digest = Digest::default();
    b.identity.byte_len = 999;
    // This pure function only checks metadata. Neither successful result authenticates either identity.
    assert_eq!(
        admit_metadata(&b).unwrap(),
        MetadataFacts {
            sensor_pixels: 24,
            active_pixels: 6
        }
    );
    b.image.cfa_layout = CfaLayout::Unsupported;
    b.metadata.cfa_layout = CfaLayout::Unsupported;
    assert_eq!(admit_metadata(&b), Err(Refusal::CfaLayout));
}

#[test]
fn invalid_sensor_count_dimensions_and_crop_refuse_without_allocating() {
    let mut d = decoded();
    d.image.data.pop();
    assert_eq!(admit_metadata(&d), Err(Refusal::Dimensions));
    let mut d = decoded();
    d.metadata.width = 5;
    assert_eq!(admit_metadata(&d), Err(Refusal::Dimensions));
    for crop in [
        [0, 0, 0, 1],
        [6, 0, 1, 1],
        [u32::MAX, 0, 2, 1],
        [0, 3, 1, 2],
    ] {
        let mut d = decoded();
        d.metadata.default_crop = crop;
        assert_eq!(admit_metadata(&d), Err(Refusal::Crop));
    }
}

#[test]
fn xtrans_invalid_bayer_and_layout_mismatch_refuse() {
    for layout in [
        CfaLayout::XTrans([[1; 6]; 6]),
        CfaLayout::Bayer([[0, 1], [4, 2]]),
        CfaLayout::Bayer([[0; 2]; 2]),
    ] {
        let mut d = decoded();
        d.image.cfa_layout = layout;
        d.metadata.cfa_layout = layout;
        assert_eq!(admit_metadata(&d), Err(Refusal::CfaLayout));
    }
    let mut d = decoded();
    d.metadata.cfa_layout = CfaLayout::Bayer([[2, 3], [1, 0]]);
    assert_eq!(admit_metadata(&d), Err(Refusal::CfaLayout));
}

#[test]
fn unsupported_orientation_and_correction_metadata_refuse() {
    for orientation in 0..=9 {
        if orientation == 1 {
            continue;
        }
        let mut d = decoded();
        d.metadata.orientation = orientation;
        assert_eq!(admit_metadata(&d), Err(Refusal::Orientation));
    }
    let mut d = decoded();
    d.metadata.has_gain_map = true;
    assert_eq!(admit_metadata(&d), Err(Refusal::CorrectionMetadata));
    let mut d = decoded();
    d.metadata.has_opcode_list = true;
    assert_eq!(admit_metadata(&d), Err(Refusal::CorrectionMetadata));
    let mut d = decoded();
    d.metadata.opcode_lists[2] = Some(Vec::new());
    assert_eq!(admit_metadata(&d), Err(Refusal::CorrectionMetadata));
}

#[test]
fn malformed_sensor_levels_wb_and_matrices_refuse() {
    for channel in 0..4 {
        for black in [f32::NAN, f32::INFINITY, 4095.0, 4096.0] {
            let mut d = decoded();
            d.metadata.black_levels[channel] = black;
            assert_eq!(admit_metadata(&d), Err(Refusal::Calibration));
        }
    }
    let mut d = decoded();
    d.metadata.white_level = 0;
    assert_eq!(admit_metadata(&d), Err(Refusal::Calibration));
    for channel in 0..3 {
        for wb in [0.0, -1.0, f32::NAN, f32::INFINITY] {
            let mut d = decoded();
            d.metadata.as_shot_wb[channel] = wb;
            assert_eq!(admit_metadata(&d), Err(Refusal::Calibration));
        }
    }
    // Native2 inverts these first three rows, independently of camera_to_xyz.
    for row in 0..3 {
        for column in 0..3 {
            for invalid in [f32::NAN, f32::INFINITY] {
                let mut d = decoded();
                d.metadata.cam_xyz[row][column] = invalid;
                assert_eq!(admit_metadata(&d), Err(Refusal::Calibration));
            }
        }
    }
    let mut d = decoded();
    d.metadata.cam_xyz = [[0.0; 3]; 4];
    assert_eq!(admit_metadata(&d), Err(Refusal::Calibration));
    let mut d = decoded();
    d.metadata.cam_xyz[1] = d.metadata.cam_xyz[0];
    assert_eq!(admit_metadata(&d), Err(Refusal::Calibration));

    let mut d = decoded();
    d.metadata.camera_to_xyz.0[1][1] = f64::NAN;
    assert_eq!(admit_metadata(&d), Err(Refusal::Calibration));
    let mut d = decoded();
    d.metadata.camera_to_xyz.0 = [[0.0; 3]; 3];
    assert_eq!(admit_metadata(&d), Err(Refusal::Calibration));
}

fn request() -> AllocationInventory {
    AllocationInventory {
        sensor_pixels: 24,
        active_pixels: 6,
        metadata_bytes: 16,
        scratch_bytes: 32,
    }
}
fn limits() -> CheckedLimits {
    CheckedLimits {
        sensor_pixels: 24,
        active_pixels: 6,
        output_bytes: 48,
        accounted_bytes: 240,
    }
}

// Break caught: unchecked sum, omitted U16+F32 coexistence, output overflow or off-by-one cap.
#[test]
fn checked_inventory_accepts_exact_limit_and_rejects_each_lower_limit() {
    // 24*(2+4) + 6*8 + 16 + 32 = 240; not a proven full renderer peak.
    assert_eq!(
        check_inventory(request(), limits()),
        Ok(AccountedBytes {
            output: 48,
            total: 240
        })
    );
    for cap in [
        CheckedLimits {
            sensor_pixels: 23,
            ..limits()
        },
        CheckedLimits {
            active_pixels: 5,
            ..limits()
        },
        CheckedLimits {
            output_bytes: 47,
            ..limits()
        },
        CheckedLimits {
            accounted_bytes: 239,
            ..limits()
        },
    ] {
        assert_eq!(check_inventory(request(), cap), Err(Refusal::Budget));
    }
}

#[test]
fn checked_inventory_rejects_zero_and_overflow_with_no_large_buffers() {
    for inventory in [
        AllocationInventory {
            sensor_pixels: 0,
            ..request()
        },
        AllocationInventory {
            active_pixels: 0,
            ..request()
        },
        AllocationInventory {
            active_pixels: 25,
            ..request()
        },
    ] {
        assert_eq!(
            check_inventory(inventory, limits()),
            Err(Refusal::Dimensions)
        );
    }
    let unlimited_numbers = CheckedLimits {
        sensor_pixels: u64::MAX,
        active_pixels: u64::MAX,
        output_bytes: u64::MAX,
        accounted_bytes: u64::MAX,
    };
    for inventory in [
        AllocationInventory {
            sensor_pixels: u64::MAX,
            active_pixels: 1,
            ..request()
        },
        AllocationInventory {
            sensor_pixels: u64::MAX / 6,
            active_pixels: u64::MAX / 8 + 1,
            ..request()
        },
        AllocationInventory {
            scratch_bytes: u64::MAX,
            ..request()
        },
        AllocationInventory {
            metadata_bytes: u64::MAX,
            ..request()
        },
    ] {
        assert_eq!(
            check_inventory(inventory, unlimited_numbers),
            Err(Refusal::Overflow)
        );
    }
}

#[test]
fn neural_models_local_edits_depth_lut_and_hdr_are_refused_without_mutation() {
    use engine_api::id::ModelRef;
    use engine_api::recipe::mask::LocalAdjustment;
    use engine_api::recipe::settings::{DenoiseMethod, LensBlur, LutSettings};
    let mut cases = Vec::new();
    let mut r = narrow_recipe();
    r.settings.denoise.method = DenoiseMethod::Neural {
        model: ModelRef::default(),
        joint_demosaic: false,
    };
    cases.push((r, Refusal::Denoise));
    let mut r = narrow_recipe();
    r.settings.demosaic.model = Some(ModelRef::default());
    cases.push((r, Refusal::Demosaic));
    let mut r = narrow_recipe();
    r.settings
        .locals
        .adjustments
        .push(LocalAdjustment::default());
    cases.push((r, Refusal::LocalEdits));
    let mut r = narrow_recipe();
    r.settings.effects.lens_blur = Some(LensBlur::default());
    cases.push((r, Refusal::Depth));
    let mut r = narrow_recipe();
    r.settings.color.lut = Some(LutSettings::default());
    cases.push((r, Refusal::ColorDependency));
    let mut r = narrow_recipe();
    r.settings.output.hdr = true;
    cases.push((r, Refusal::Output));
    let mut r = narrow_recipe();
    r.settings.output.hdr_headroom_stops = 1.0;
    cases.push((r, Refusal::Output));
    for (recipe, expected) in cases {
        let d = from_recipe(&recipe);
        let before = d.recipe_json().to_vec();
        assert_eq!(admit_recipe(&d).unwrap_err(), expected);
        assert_eq!(d.recipe_json(), before);
    }
}

#[test]
fn all_bayer_phases_and_both_green_conventions_remain_admissible() {
    for (g1, g2) in [(1, 1), (1, 3), (3, 1), (3, 3)] {
        for phase in [
            [[0, g1], [g2, 2]],
            [[g1, 0], [2, g2]],
            [[g2, 2], [0, g1]],
            [[2, g2], [g1, 0]],
        ] {
            let mut d = decoded();
            d.image.cfa_layout = CfaLayout::Bayer(phase);
            d.metadata.cfa_layout = d.image.cfa_layout;
            // RGB white balance consumes only the first three multipliers.
            d.metadata.as_shot_wb[3] = 0.0;
            assert_eq!(
                admit_metadata(&d),
                Ok(MetadataFacts {
                    sensor_pixels: 24,
                    active_pixels: 6
                })
            );
        }
    }
}

#[test]
fn every_non_neutral_lens_member_refuses_including_inactive_ranges() {
    use engine_api::recipe::settings::LensSettings;
    let changes: &[fn(&mut LensSettings)] = &[
        |l| l.profile = LensProfileSource::Embedded,
        |l| l.profile = LensProfileSource::AutoCalibrated,
        |l| l.distortion_scale = 99.0,
        |l| l.vignetting_scale = 99.0,
        |l| l.chromatic_aberration_scale = 99.0,
        |l| l.remove_chromatic_aberration = true,
        |l| l.manual_distortion = 1.0,
        |l| l.manual_vignetting = 1.0,
        |l| l.manual_vignetting_midpoint = 49.0,
        |l| l.defringe_purple.amount = 1.0,
        |l| l.defringe_purple.hue_range[0] = 269.0,
        |l| l.defringe_green.amount = 1.0,
        |l| l.defringe_green.hue_range[1] = 111.0,
        |l| l.softness_correction = 1.0,
    ];
    for change in changes {
        let mut r = narrow_recipe();
        change(&mut r.settings.lens);
        let d = from_recipe(&r);
        let before = d.recipe_json().to_vec();
        assert_eq!(admit_recipe(&d).unwrap_err(), Refusal::Lens);
        assert_eq!(d.recipe_json(), before);
    }
}

#[test]
fn retouch_point_color_proof_and_even_disabled_local_inputs_refuse() {
    use engine_api::color::IccProfileHandle;
    use engine_api::id::RetouchId;
    use engine_api::recipe::mask::{LocalAdjustment, RetouchKind, RetouchOperation, RetouchTarget};
    use engine_api::recipe::settings::PointColor;
    let mut cases = Vec::new();
    let mut r = narrow_recipe();
    r.settings.locals.retouch.push(RetouchOperation {
        id: RetouchId(1),
        kind: RetouchKind::Clone {
            source_offset: [0.0, 0.0],
        },
        target: RetouchTarget::Implicit,
        opacity: 100.0,
        feather: 0.0,
        enabled: false,
    });
    cases.push((r, Refusal::LocalEdits));
    let mut r = narrow_recipe();
    r.settings.locals.adjustments.push(LocalAdjustment {
        enabled: false,
        ..Default::default()
    });
    cases.push((r, Refusal::LocalEdits));
    let mut r = narrow_recipe();
    r.settings.color.point_colors.push(PointColor::default());
    cases.push((r, Refusal::ColorDependency));
    let mut r = narrow_recipe();
    r.settings.output.proof_profile = Some(IccProfileHandle(Digest::default()));
    cases.push((r, Refusal::Output));
    for (r, expected) in cases {
        let d = from_recipe(&r);
        let before = d.recipe_json().to_vec();
        assert_eq!(admit_recipe(&d).unwrap_err(), expected);
        assert_eq!(d.recipe_json(), before);
    }
}

#[test]
fn unsupported_decode_reconstruction_and_display_controls_are_not_sanitized() {
    use engine_api::recipe::settings::{
        DevelopSettings, DisplayTransform, HighlightReconstruction, WhiteBalanceMode,
    };
    let changes: &[fn(&mut DevelopSettings)] = &[
        |s| s.white_balance.mode = WhiteBalanceMode::Auto,
        |s| s.decode.frame_index = 1,
        |s| s.decode.pixel_shift_merge = true,
        |s| s.linearize.highlight_reconstruction = HighlightReconstruction::ReconstructLch,
        |s| s.linearize.highlight_reconstruction = HighlightReconstruction::Inpaint,
        |s| s.tone.display_transform = DisplayTransform::Agx,
        |s| s.tone.display_transform = DisplayTransform::Filmic,
        |s| s.tone.display_transform = DisplayTransform::AdobePv6Compat,
    ];
    for change in changes {
        let mut r = narrow_recipe();
        change(&mut r.settings);
        let d = from_recipe(&r);
        let before = d.recipe_json().to_vec();
        assert_eq!(admit_recipe(&d).unwrap_err(), Refusal::UnsupportedSettings);
        assert_eq!(d.recipe_json(), before);
    }
}

// This exercises existing descriptor authority: nonneutral geometry cannot produce
// a descriptor for admit_recipe. Do not fabricate private wire state to bypass it.
#[test]
fn descriptor_rejects_every_non_neutral_geometry_member_without_rewriting_recipe() {
    use engine_api::recipe::settings::{GeometrySettings, GuideLine, UprightMode};
    let changes: &[fn(&mut GeometrySettings)] = &[
        |g| g.orientation = 2,
        |g| g.constrain_crop = true,
        |g| g.crop.rect.left = 0.1,
        |g| g.crop.rect.top = 0.1,
        |g| g.crop.rect.right = 0.9,
        |g| g.crop.rect.bottom = 0.9,
        |g| g.crop.angle = 1.0,
        |g| g.crop.aspect = Some([3, 2]),
        |g| g.upright.mode = UprightMode::Auto,
        |g| g.upright.mode = UprightMode::Guided,
        |g| {
            g.upright.guides.push(GuideLine {
                start: [0.1, 0.1],
                end: [0.1, 0.9],
            })
        },
        |g| g.transform.vertical = 1.0,
        |g| g.transform.horizontal = 1.0,
        |g| g.transform.rotate = 1.0,
        |g| g.transform.aspect = 1.0,
        |g| g.transform.scale = 99.0,
        |g| g.transform.offset_x = 0.1,
        |g| g.transform.offset_y = 0.1,
    ];
    for change in changes {
        let mut recipe = narrow_recipe();
        change(&mut recipe.settings.geometry);
        let original_bytes = recipe.to_json().unwrap();
        let supplied = original_bytes.clone();
        let result = PinnedRawDescriptor::new(PinnedRawInput {
            asset_digest: Digest::default(),
            asset_byte_len: 9,
            recipe_image_id: ImageId(1),
            recipe_json: supplied,
            decoder_route: PinnedRawDecoderRoute::LibRawCfaV1,
            suffix_hint: "arw".into(),
            locator_hint: None,
        });
        assert!(
            matches!(result, Err(engine_api::EngineError::InvalidArgument { name, .. }) if name == "settings.geometry")
        );
        assert_eq!(recipe.to_json().unwrap(), original_bytes);
    }
    let neutral = from_recipe(&narrow_recipe());
    let original_bytes = neutral.recipe_json().to_vec();
    assert_eq!(
        admit_recipe(&neutral).unwrap().settings.geometry,
        GeometrySettings::default()
    );
    assert_eq!(neutral.recipe_json(), original_bytes);
}

#[test]
fn invertible_calibration_with_nonpositive_as_shot_scene_white_refuses() {
    let mut d = decoded();
    // Nonsingular diag(-1,1,1) inverts successfully but maps the positive camera
    // white to negative X. This must fail independently of stored camera_to_xyz.
    d.metadata.cam_xyz[0][0] = -1.0;
    let matrix = ColorMatrix3([[-1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]);
    assert!(pipeline_cpu::camera_to_xyz(matrix).is_ok());
    assert_eq!(admit_metadata(&d), Err(Refusal::Calibration));
    assert_eq!(
        admit_metadata(&decoded()),
        Ok(MetadataFacts {
            sensor_pixels: 24,
            active_pixels: 6
        })
    );
}
