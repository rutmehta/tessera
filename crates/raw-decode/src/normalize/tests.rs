use super::*;

fn plane(width: u32, height: u32, layout: CfaLayout) -> PackedPlane {
    PackedPlane {
        width,
        height,
        layout,
        samples: (0..u64::from(width) * u64::from(height))
            .map(|i| [0, 63, 100, 256, 1023, 1024, 2048, u16::MAX][i as usize % 8])
            .collect(),
        black: [64.0, 32.0, 16.0, 8.0],
        white: 1024,
    }
}
fn bayer() -> CfaLayout {
    CfaLayout::Bayer([[0, 1], [3, 2]])
}
fn expected(p: &PackedPlane) -> Vec<u32> {
    // Independent scalar reference: explicit full-sensor lookup, not channel_at/helper.
    p.samples
        .iter()
        .enumerate()
        .map(|(i, &v)| {
            let x = i % p.width as usize;
            let y = i / p.width as usize;
            let c = match p.layout {
                CfaLayout::Bayer(m) => m[y % 2][x % 2],
                CfaLayout::XTrans(m) => m[y % 6][x % 6],
                CfaLayout::Unsupported => panic!("oracle requires supported layout"),
            } as usize;
            let numerator = v as f32 - p.black[c];
            let denominator = p.white as f32 - p.black[c];
            (numerator / denominator).clamp(0.0, 1.2).to_bits()
        })
        .collect()
}
fn reserve(v: &mut Vec<f32>, count: usize) -> EngineResult<()> {
    v.try_reserve_exact(count).map_err(|e| EngineError::Decode {
        format: "raw".into(),
        message: format!("normalization allocation: {e}"),
    })
}
fn error(result: EngineResult<CfaImage>) -> EngineError {
    match result {
        Ok(_) => panic!("unexpected published image"),
        Err(e) => e,
    }
}
fn is_decode(e: &EngineError) -> bool {
    matches!(e, EngineError::Decode { format, .. } if format == "raw")
}
fn assert_exact(p: PackedPlane) {
    let want = expected(&p);
    let (w, h) = (p.width, p.height);
    let got = normalize(p, &CancellationToken::new()).unwrap();
    assert_eq!(got.pyramid.extent.width, w);
    assert_eq!(got.pyramid.extent.height, h);
    assert_eq!(
        got.pyramid
            .pixels
            .iter()
            .map(|v| v.to_bits())
            .collect::<Vec<_>>(),
        want
    );
}

#[test]
fn bayer_all_phases_and_green_conventions_are_bit_exact() {
    for (g1, g2) in [(1, 1), (1, 3), (3, 1), (3, 3)] {
        for m in [
            [[0, g1], [g2, 2]],
            [[g1, 0], [2, g2]],
            [[g2, 2], [0, g1]],
            [[2, g2], [g1, 0]],
        ] {
            assert_exact(plane(9, 7, CfaLayout::Bayer(m)));
        }
    }
}
#[test]
fn xtrans_all_phases_are_bit_exact() {
    let base = [
        [1, 0, 1, 1, 2, 1],
        [2, 1, 2, 0, 1, 0],
        [1, 0, 1, 1, 2, 1],
        [1, 2, 1, 1, 0, 1],
        [0, 1, 0, 2, 1, 2],
        [1, 2, 1, 1, 0, 1],
    ];
    for dy in 0..6 {
        for dx in 0..6 {
            let m =
                std::array::from_fn(|y| std::array::from_fn(|x| base[(y + dy) % 6][(x + dx) % 6]));
            assert_exact(plane(13, 11, CfaLayout::XTrans(m)));
        }
    }
}
#[test]
fn normalization_is_once_and_retains_clamped_above_white_samples() {
    let mut p = plane(4, 2, bayer());
    p.black = [0.; 4];
    p.samples = vec![0, 256, 512, 1024, 1280, 2048, 1, 1023];
    let got = normalize(p, &CancellationToken::new()).unwrap();
    assert_eq!(got.pyramid.pixels[1].to_bits(), 0.25f32.to_bits());
    assert_eq!(got.pyramid.pixels[3].to_bits(), 1.0f32.to_bits());
    assert_eq!(got.pyramid.pixels[4].to_bits(), 1.2f32.to_bits());
    assert_eq!(got.pyramid.pixels[5].to_bits(), 1.2f32.to_bits());
}
#[test]
fn offset_crop_does_not_shift_full_sensor_phase() {
    let mut p = plane(9, 7, bayer());
    p.samples.fill(512);
    let want = expected(&p);
    let black = p.black;
    let got = normalize(p, &CancellationToken::new()).unwrap();
    let (left, top, width, height) = (1usize, 1usize, 5usize, 3usize);
    for y in 0..height {
        for x in 0..width {
            let i = (top + y) * 9 + left + x;
            assert_eq!(got.pyramid.pixels[i].to_bits(), want[i]);
        }
    }
    // Full sensor (1,1) is B, whereas crop-relative (0,0) would be R.
    let wrong = ((512f32 - black[0]) / (1024f32 - black[0])).to_bits();
    assert_ne!(got.pyramid.pixels[10].to_bits(), wrong);
}
#[test]
fn finite_negative_black_and_legacy_zero_white_remain_compatible() {
    let mut p = plane(5, 3, bayer());
    p.black = [-64., -32., -16., -8.];
    assert_exact(p);
    let mut p = plane(2, 2, bayer());
    p.black = [-2.; 4];
    p.white = 0;
    assert_exact(p);
}
#[test]
fn legacy_channel_range_layouts_are_not_replaced_by_stricter_profile() {
    assert_exact(plane(3, 3, CfaLayout::Bayer([[0, 0], [0, 0]])));
    assert_exact(plane(7, 7, CfaLayout::XTrans([[2; 6]; 6])));
}
#[test]
fn invalid_dimensions_counts_layout_and_levels_refuse_before_reserve() {
    let mut cases = Vec::new();
    let mut p = plane(2, 2, bayer());
    p.width = 0;
    cases.push(p);
    let mut p = plane(2, 2, bayer());
    p.height = 0;
    cases.push(p);
    let mut p = plane(2, 2, bayer());
    p.samples.pop();
    cases.push(p);
    let mut p = plane(2, 2, bayer());
    p.samples.push(1);
    cases.push(p);
    cases.push(plane(2, 2, CfaLayout::Unsupported));
    cases.push(plane(2, 2, CfaLayout::Bayer([[4, 1], [1, 2]])));
    cases.push(plane(2, 2, CfaLayout::XTrans([[3; 6]; 6])));
    for channel in 0..4 {
        for value in [f32::NAN, f32::INFINITY, 1024., 1025.] {
            let mut p = plane(2, 2, bayer());
            p.black[channel] = value;
            cases.push(p);
        }
    }
    for p in cases {
        let mut reserved = 0;
        let mut events = Vec::new();
        let mut alloc = |_: &mut Vec<f32>, _: usize| {
            reserved += 1;
            Ok(())
        };
        let mut observe = |e| events.push(e);
        let got = normalize_observed(
            p,
            &CancellationToken::new(),
            &mut Hooks {
                reserve: &mut alloc,
                observe: &mut observe,
            },
        );
        assert!(is_decode(&error(got)));
        assert_eq!(reserved, 0);
        assert!(events.is_empty());
    }
}
#[test]
fn checked_sizes_accept_exact_count_without_allocating() {
    assert_eq!(
        checked_sizes(9, 7, 63).unwrap(),
        Sizes {
            samples: 63,
            output_bytes: 252
        }
    );
}
#[test]
fn checked_sizes_reject_extent_count_and_allocation_overflow() {
    for (w, h, len) in [
        (0, 1, 0),
        (1, 0, 0),
        (2, 2, 3),
        (2, 2, 5),
        (u32::MAX, u32::MAX, 0),
    ] {
        let got = checked_sizes(w, h, len).unwrap_err();
        assert!(is_decode(&got));
    }
    // Explicit byte/Vec isize ceiling with no matching large allocation.
    if let Ok(len) = usize::try_from(u64::from(u32::MAX) * u64::from(u32::MAX)) {
        assert!(is_decode(
            &checked_sizes(u32::MAX, u32::MAX, len).unwrap_err()
        ));
    }
}
#[test]
fn pre_cancelled_request_allocates_and_publishes_nothing() {
    let cancel = CancellationToken::new();
    cancel.cancel();
    let mut calls = 0;
    let mut alloc = |_: &mut Vec<f32>, _: usize| {
        calls += 1;
        Ok(())
    };
    let mut events = Vec::new();
    let mut observe = |e| events.push(e);
    let result = normalize_observed(
        plane(2, 2, bayer()),
        &cancel,
        &mut Hooks {
            reserve: &mut alloc,
            observe: &mut observe,
        },
    );
    assert_eq!(error(result), EngineError::Cancelled);
    assert_eq!(calls, 0);
    assert!(events.is_empty());
}
#[test]
fn cancellation_inside_wide_row_stops_at_first_quantum() {
    let cancel = CancellationToken::new();
    let mut events = Vec::new();
    let mut observe = |e| {
        events.push(e);
        if e == (Event::Chunk {
            completed: CHECKPOINT_SAMPLES,
        }) {
            cancel.cancel();
        }
    };
    let result = normalize_observed(
        plane((CHECKPOINT_SAMPLES * 3 + 7) as u32, 1, bayer()),
        &cancel,
        &mut Hooks {
            reserve: &mut reserve,
            observe: &mut observe,
        },
    );
    assert_eq!(error(result), EngineError::Cancelled);
    assert!(events.contains(&Event::Chunk {
        completed: CHECKPOINT_SAMPLES
    }));
    assert!(
        !events
            .iter()
            .any(|e| matches!(e,Event::Chunk{completed} if *completed>CHECKPOINT_SAMPLES))
    );
    assert!(!events.contains(&Event::BeforePublish));
}
#[test]
fn cancellation_at_publication_boundary_returns_no_image() {
    let cancel = CancellationToken::new();
    let mut seen = false;
    let mut observe = |e| {
        if e == Event::BeforePublish {
            seen = true;
            cancel.cancel();
        }
    };
    let result = normalize_observed(
        plane(2, 2, bayer()),
        &cancel,
        &mut Hooks {
            reserve: &mut reserve,
            observe: &mut observe,
        },
    );
    assert_eq!(error(result), EngineError::Cancelled);
    assert!(seen);
}
#[test]
fn successful_loop_observes_bounded_chunks_including_partial_tail() {
    let n = CHECKPOINT_SAMPLES * 2 + 17;
    let mut chunks = Vec::new();
    let mut observe = |e| {
        if let Event::Chunk { completed } = e {
            chunks.push(completed);
        }
    };
    let out = normalize_observed(
        plane(n as u32, 1, bayer()),
        &CancellationToken::new(),
        &mut Hooks {
            reserve: &mut reserve,
            observe: &mut observe,
        },
    )
    .unwrap();
    assert_eq!(chunks, vec![CHECKPOINT_SAMPLES, CHECKPOINT_SAMPLES * 2, n]);
    assert_eq!(out.pyramid.pixels.len(), n);
}
#[test]
fn allocation_error_precedes_concurrent_cancellation() {
    let cancel = CancellationToken::new();
    let intended = EngineError::Decode {
        format: "raw".into(),
        message: "injected reserve failure".into(),
    };
    let mut alloc = |_: &mut Vec<f32>, _: usize| {
        cancel.cancel();
        Err(intended.clone())
    };
    let mut events = Vec::new();
    let mut observe = |e| events.push(e);
    let result = normalize_observed(
        plane(2, 2, bayer()),
        &cancel,
        &mut Hooks {
            reserve: &mut alloc,
            observe: &mut observe,
        },
    );
    assert_eq!(error(result), intended);
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, Event::Chunk { .. } | Event::BeforePublish))
    );
}
#[test]
fn owned_capacity_and_pointers_are_observed_without_clone_or_reallocation() {
    let mut p = plane(9, 7, bayer());
    p.samples.reserve_exact(100);
    let input_ptr = p.samples.as_ptr() as usize;
    let input_capacity = p.samples.capacity();
    let want = expected(&p);
    let mut events = Vec::new();
    let mut observe = |e| events.push(e);
    let mut alloc = |out: &mut Vec<f32>, n: usize| reserve(out, n + 31);
    let got = normalize_observed(
        p,
        &CancellationToken::new(),
        &mut Hooks {
            reserve: &mut alloc,
            observe: &mut observe,
        },
    )
    .unwrap();
    let facts = events
        .iter()
        .find_map(|e| {
            if let Event::Reserved {
                input_ptr,
                input_capacity,
                output_ptr,
                output_capacity,
            } = e
            {
                Some((*input_ptr, *input_capacity, *output_ptr, *output_capacity))
            } else {
                None
            }
        })
        .expect("actual post-reserve observation");
    assert_eq!(facts.0, input_ptr);
    assert_eq!(facts.1, input_capacity);
    assert_eq!(facts.2, got.pyramid.pixels.as_ptr() as usize);
    assert_eq!(facts.3, got.pyramid.pixels.capacity());
    assert!(facts.3 >= 94);
    assert!(facts.1.checked_mul(2).unwrap() + facts.3.checked_mul(4).unwrap() > 63 * 6);
    assert_eq!(
        got.pyramid
            .pixels
            .iter()
            .map(|v| v.to_bits())
            .collect::<Vec<_>>(),
        want
    );
}

#[test]
fn cancellation_after_validation_and_reservation_prevents_conversion() {
    for after_reserve in [false, true] {
        let cancel = CancellationToken::new();
        let mut allocations = 0;
        let mut events = Vec::new();
        let mut alloc = |out: &mut Vec<f32>, count: usize| {
            allocations += 1;
            reserve(out, count)
        };
        let mut observe = |event| {
            events.push(event);
            if (!after_reserve && event == Event::Validated)
                || (after_reserve && matches!(event, Event::Reserved { .. }))
            {
                cancel.cancel();
            }
        };
        let result = normalize_observed(
            plane(9, 7, bayer()),
            &cancel,
            &mut Hooks {
                reserve: &mut alloc,
                observe: &mut observe,
            },
        );
        assert_eq!(error(result), EngineError::Cancelled);
        assert_eq!(allocations, usize::from(after_reserve));
        assert!(events.contains(&Event::Validated));
        assert_eq!(
            events.iter().any(|e| matches!(e, Event::Reserved { .. })),
            after_reserve
        );
        assert!(
            !events
                .iter()
                .any(|e| matches!(e, Event::Chunk { .. } | Event::BeforePublish))
        );
    }
}

#[test]
fn checked_sizes_reject_isize_ceiling_without_usize_arithmetic_overflow() {
    // Pure dimensions/count only: no giant Vec. 4*S fits usize but exceeds
    // Vec's isize::MAX byte ceiling on each explicitly supported pointer width.
    #[cfg(target_pointer_width = "64")]
    let (width, height, count) = (1u32 << 31, 1u32 << 30, 1usize << 61);
    #[cfg(target_pointer_width = "32")]
    let (width, height, count) = (1u32 << 29, 1u32, 1usize << 29);
    #[cfg(not(any(target_pointer_width = "32", target_pointer_width = "64")))]
    compile_error!("normalization capacity contract requires an explicit pointer-width case");
    assert_eq!(u64::from(width) * u64::from(height), count as u64);
    let bytes = count.checked_mul(std::mem::size_of::<f32>()).unwrap();
    assert!(bytes > isize::MAX as usize);
    assert!(is_decode(&checked_sizes(width, height, count).unwrap_err()));
}
