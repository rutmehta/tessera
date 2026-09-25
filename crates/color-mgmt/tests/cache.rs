use color_mgmt::*;
use std::sync::Arc;

#[test]
fn registry_reuses_lut_across_transform_lifetimes_and_profile_handles() {
    let mut registry = Registry::new();
    let source = registry.builtin(Builtin::Srgb).unwrap();
    let destination = registry.builtin(Builtin::DisplayP3).unwrap();
    let first = Transform::new(&source, &destination, Default::default())
        .unwrap()
        .lut33();
    // A distinct handle with identical ICC contents must address the same entry.
    let mut other = Registry::new();
    let destination_copy = other.load_bytes(destination.icc_bytes()).unwrap();
    let second = Transform::new(&source, &destination_copy, Default::default())
        .unwrap()
        .lut33();
    assert!(Arc::ptr_eq(&first, &second));
}

#[test]
fn cache_separates_every_profile_and_option() {
    let mut r = Registry::new();
    let s = r.builtin(Builtin::Srgb).unwrap();
    let p = r.builtin(Builtin::DisplayP3).unwrap();
    let base = TransformOptions::default();
    let variants = [
        base,
        TransformOptions {
            intent: Intent::Perceptual,
            ..base
        },
        TransformOptions {
            intent: Intent::Saturation,
            ..base
        },
        TransformOptions {
            intent: Intent::AbsoluteColorimetric,
            ..base
        },
        TransformOptions {
            black_point_compensation: false,
            ..base
        },
        TransformOptions {
            simulate_paper: true,
            ..base
        },
        TransformOptions {
            gamut_threshold: 3.0,
            ..base
        },
    ];
    let mut luts = Vec::new();
    for source in [&s, &p] {
        for destination in [&s, &p] {
            for proof in [None, Some(&s), Some(&p)] {
                for options in variants {
                    let make = || {
                        match proof {
                            None => Transform::new(source, destination, options),
                            Some(proof) => Transform::proof(source, destination, proof, options),
                        }
                        .unwrap()
                        .lut33()
                    };
                    let lut = make();
                    assert!(Arc::ptr_eq(&lut, &make()), "identical key not reused");
                    assert!(
                        luts.iter().all(|old| !Arc::ptr_eq(old, &lut)),
                        "distinct keys aliased"
                    );
                    luts.push(lut);
                }
            }
        }
    }
}

#[test]
fn concurrent_transform_creation_shares_one_lut() {
    let s = Registry::new().builtin(Builtin::Srgb).unwrap();
    let barrier = Arc::new(std::sync::Barrier::new(4));
    let threads: Vec<_> = (0..4)
        .map(|_| {
            let s = s.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                let t = Transform::new(&s, &s, Default::default()).unwrap();
                barrier.wait();
                t.lut33()
            })
        })
        .collect();
    let luts: Vec<_> = threads.into_iter().map(|t| t.join().unwrap()).collect();
    assert!(luts.iter().all(|lut| Arc::ptr_eq(&luts[0], lut)));
}
