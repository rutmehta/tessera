use ml_faces::{FaceQuality, QualityGate, cluster, cluster_eligible, cluster_with_medoids};

#[test]
fn incremental_agrees_with_batch_on_known_identities() {
    use ml_faces::nearest_medoid;
    let (data, truth) = identities(8, 40);
    let base = cluster_with_medoids(&data[..160], 0.7).unwrap();
    let medoids: Vec<_> = base.clusters.iter().map(|c| c.medoid).collect();
    let mut groups: Vec<_> = base.clusters.iter().map(|c| c.members.clone()).collect();
    for (i, row) in data.iter().enumerate().skip(160) {
        let matched = nearest_medoid(row, &medoids, 0.7).unwrap().unwrap();
        assert!(matched.similarity >= 0.7);
        groups[matched.medoid_index].push(i);
    }
    let batch = cluster(&data, 0.7).unwrap();
    assert_eq!(groups, batch);
    assert!(ari(&truth, &groups) > 0.95);
    let mut unseen = [0.; 128];
    unseen[127] = 1.;
    assert!(nearest_medoid(&unseen, &medoids, 0.7).unwrap().is_none());
    assert!(nearest_medoid(&unseen, &[], 0.7).unwrap().is_none());
    assert!(nearest_medoid(&[0.; 128], &medoids, 0.7).is_err());
    assert!(nearest_medoid(&unseen, &[[0.; 128]], 0.7).is_err());
    assert!(nearest_medoid(&unseen, &[], f32::NAN).is_err());
    assert_eq!(
        nearest_medoid(&medoids[0], &[medoids[0], medoids[0]], 0.7)
            .unwrap()
            .unwrap()
            .medoid_index,
        0
    );
}

#[test]
fn bounded_sample_keeps_partition_and_identity_quality() {
    let (data, truth) = identities(12, 100);
    let result = cluster_with_medoids(&data, 0.7).unwrap();
    assert!(result.approximate);
    let groups: Vec<_> = result.clusters.iter().map(|c| c.members.clone()).collect();
    let mut members: Vec<_> = groups.iter().flatten().copied().collect();
    members.sort_unstable();
    assert_eq!(members, (0..data.len()).collect::<Vec<_>>());
    assert!(ari(&truth, &groups) > 0.95);
    assert_eq!(result, cluster_with_medoids(&data, 0.7).unwrap());
}

#[test]
#[ignore = "100k release performance acceptance; run explicitly"]
fn benchmark_100k_under_30_seconds() {
    let (data, truth) = identities(100, 1000);
    let start = std::time::Instant::now();
    let result = cluster_with_medoids(&data, 0.7).unwrap();
    let elapsed = start.elapsed();
    let groups: Vec<_> = result.clusters.into_iter().map(|c| c.members).collect();
    let score = ari(&truth, &groups);
    println!(
        "100000 embeddings: {elapsed:?}, clusters={}, ARI={score}, approximate={}",
        groups.len(),
        result.approximate
    );
    assert!(result.approximate);
    assert!(score > 0.95);
    assert!(
        elapsed.as_secs_f64() < 30.0,
        "100k clustering exceeded 30 seconds"
    );
}

#[test]
fn identical_descriptors_match_at_inclusive_one_threshold() {
    let row = [1.; 128];
    let matched = ml_faces::nearest_medoid(&row, &[row], 1.).unwrap();
    assert!(matched.is_some());
    assert_eq!(cluster(&[row, row], 1.).unwrap(), vec![vec![0, 1]]);
}

fn identities(count: usize, each: usize) -> (Vec<[f32; 128]>, Vec<usize>) {
    let mut data = Vec::new();
    let mut labels = Vec::new();
    let mut seed = 41u64;
    for j in 0..each {
        for identity in 0..count {
            let mut row = [0.; 128];
            for value in &mut row {
                seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
                *value = ((seed >> 32) as f32 / u32::MAX as f32 - 0.5)
                    * (0.01 + j as f32 / each as f32 * 0.03);
            }
            row[identity] += 1.;
            data.push(row);
            labels.push(identity);
        }
    }
    (data, labels)
}
fn ari(a: &[usize], groups: &[Vec<usize>]) -> f64 {
    use std::collections::BTreeMap;
    let mut cells = BTreeMap::new();
    let mut rows = BTreeMap::new();
    let mut cols = BTreeMap::new();
    for (g, members) in groups.iter().enumerate() {
        for &i in members {
            *cells.entry((a[i], g)).or_insert(0usize) += 1;
            *rows.entry(a[i]).or_insert(0usize) += 1;
            *cols.entry(g).or_insert(0usize) += 1;
        }
    }
    let pairs = |n: usize| (n * n.saturating_sub(1)) as f64 / 2.;
    let sum = |m: Vec<usize>| m.into_iter().map(pairs).sum::<f64>();
    let joint = sum(cells.into_values().collect());
    let row = sum(rows.into_values().collect());
    let col = sum(cols.into_values().collect());
    let expected = row * col / pairs(a.len());
    (joint - expected) / ((row + col) / 2. - expected)
}
#[test]
fn known_identity_ari_and_real_member_medoids() {
    let (data, truth) = identities(12, 30);
    let result = cluster_with_medoids(&data, 0.7).unwrap();
    let groups: Vec<_> = result.clusters.iter().map(|c| c.members.clone()).collect();
    let score = ari(&truth, &groups);
    println!("known-identity synthetic ARI={score}");
    assert!(score > 0.95);
    assert!(!result.approximate);
    for c in &result.clusters {
        assert!(c.members.contains(&c.medoid_index));
        assert!(c.eligible);
        let norm = data[c.medoid_index]
            .iter()
            .map(|v| v * v)
            .sum::<f32>()
            .sqrt();
        for (x, y) in c.medoid.iter().zip(data[c.medoid_index]) {
            assert!((x - y / norm).abs() < 1e-5);
        }
    }
    assert_eq!(cluster(&data, 0.7).unwrap(), groups);
}

#[test]
fn quality_exclusion_preserves_original_ordinals() {
    let (mut data, _) = identities(2, 8);
    let good = FaceQuality {
        confidence: 0.99,
        width: 80.,
        height: 90.,
        sharpness: 0.8,
    };
    let mut quality = vec![good; data.len()];
    quality[0].width = 8.;
    quality[3].sharpness = 0.;
    quality[6].confidence = 0.2;
    quality[9].height = f32::NAN;
    // Excluded faces may not have usable embeddings and must never seed identities.
    data[0] = [0.; 128];
    let gate = QualityGate::default();
    assert!(gate.eligible(good).unwrap());
    let result = cluster_eligible(&data, &quality, 0.7, gate).unwrap();
    let mut members: Vec<_> = result
        .clusters
        .iter()
        .flat_map(|c| c.members.clone())
        .collect();
    members.sort_unstable();
    assert_eq!(members, (0..data.len()).collect::<Vec<_>>());
    for i in [0, 3, 6, 9] {
        assert!(!result.eligibility[i]);
        let c = result
            .clusters
            .iter()
            .find(|c| c.members.contains(&i))
            .unwrap();
        assert_eq!(c.members, vec![i]);
        assert!(!c.eligible);
    }
    assert!(cluster_eligible(&data, &quality[..2], 0.7, gate).is_err());
    assert!(
        QualityGate {
            min_sharpness: f64::NAN,
            ..gate
        }
        .eligible(good)
        .is_err()
    );
}
