use engine_api::id::ImageId;
use index::{FaceKey, FaceRecord, Index, NoopMetadataProvider, NoopSidecarReader, Query};
use ml_faces::people::{PeopleJob, PeopleOptions};

#[test]
fn missing_descriptors_remain_assignable_with_permissive_confidence_gate() {
    let (_dir, index, id) = setup();
    let mut missing = face(0, 0);
    missing.embedding = None;
    index.replace_faces(id, &[missing]).unwrap();
    let options = PeopleOptions {
        quality: ml_faces::QualityGate {
            min_confidence: 0.,
            ..Default::default()
        },
        ..Default::default()
    };
    PeopleJob::new(options)
        .unwrap()
        .run(&index, &[id], false)
        .unwrap();
    assert_eq!(index.face_assignments(id).unwrap().len(), 1);
    assert!(index.people().unwrap()[0].medoid.is_none());
}

#[test]
fn merge_rebuilds_medoid_before_incremental_assignment() {
    let (dir, mut index, id) = setup();
    index.replace_faces(id, &[face(0, 0), face(1, 0)]).unwrap();
    index.create_person("a", Some("Anna"), None).unwrap();
    index.create_person("b", None, None).unwrap();
    index
        .assign_face(
            FaceKey {
                image_id: id,
                ordinal: 0,
            },
            "a",
        )
        .unwrap();
    index
        .assign_face(
            FaceKey {
                image_id: id,
                ordinal: 1,
            },
            "b",
        )
        .unwrap();
    index.merge_people("a", "b").unwrap();
    std::fs::write(dir.path().join("b.jpg"), b"jpeg").unwrap();
    index
        .scan(dir.path(), &NoopSidecarReader, &NoopMetadataProvider)
        .unwrap();
    let other = index
        .search(&Query::default())
        .unwrap()
        .into_iter()
        .find(|i| *i != id)
        .unwrap();
    index.replace_faces(other, &[face(0, 0)]).unwrap();
    PeopleJob::new(PeopleOptions::default())
        .unwrap()
        .run(&index, &[other], false)
        .unwrap();
    assert_eq!(index.face_assignments(other).unwrap()[0].person_id, "a");
    assert!(index.people().unwrap()[0].medoid.is_some());
}

fn face(id: u32, axis: usize) -> FaceRecord {
    let mut embedding = vec![0.; 128];
    embedding[axis] = 1.;
    FaceRecord {
        id,
        bbox: [0., 0., 80., 80.],
        landmarks5: [[1., 1.]; 5],
        confidence: 1.,
        embedding: Some(embedding),
        sharpness: 0.8,
        eyes_open: Some(0.8),
    }
}
fn setup() -> (tempfile::TempDir, Index, ImageId) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.jpg"), b"jpeg").unwrap();
    let mut index = Index::open(dir.path().join("index.sqlite")).unwrap();
    index
        .scan(dir.path(), &NoopSidecarReader, &NoopMetadataProvider)
        .unwrap();
    let id = index.search(&Query::default()).unwrap()[0];
    (dir, index, id)
}
#[test]
fn job_persists_identities_names_confirmation_and_incremental_assignment() {
    let (_dir, index, id) = setup();
    index
        .replace_faces(id, &[face(0, 0), face(1, 0), face(2, 1), face(3, 1)])
        .unwrap();
    let mut job = PeopleJob::new(PeopleOptions::default()).unwrap();
    job.run(&index, &[id], false).unwrap();
    let assignments = index.face_assignments(id).unwrap();
    assert_eq!(assignments[0].person_id, assignments[1].person_id);
    assert_ne!(assignments[0].person_id, assignments[2].person_id);
    let person = assignments[0].person_id.clone();
    index.name_person(&person, Some("Anna")).unwrap();
    index
        .confirm_face(
            FaceKey {
                image_id: id,
                ordinal: 0,
            },
            true,
        )
        .unwrap();
    job.run(&index, &[id], true).unwrap();
    let after = index.face_assignments(id).unwrap();
    assert_eq!(after[0].person_id, person);
    assert_eq!(after[0].person_name.as_deref(), Some("Anna"));
    assert!(after[0].confirmed);
}

#[test]
fn incremental_and_periodic_jobs_keep_low_quality_assignable() {
    let (dir, mut index, id) = setup();
    index.replace_faces(id, &[face(0, 0), face(1, 0)]).unwrap();
    let mut job = PeopleJob::new(PeopleOptions {
        recluster_every: 2,
        ..Default::default()
    })
    .unwrap();
    job.run(&index, &[id], false).unwrap();
    let person = index.face_assignments(id).unwrap()[0].person_id.clone();
    std::fs::write(dir.path().join("b.jpg"), b"jpeg").unwrap();
    index
        .scan(dir.path(), &NoopSidecarReader, &NoopMetadataProvider)
        .unwrap();
    let other = index
        .search(&Query::default())
        .unwrap()
        .into_iter()
        .find(|i| *i != id)
        .unwrap();
    index.replace_faces(other, &[face(0, 0)]).unwrap();
    let report = job.run(&index, &[id, other], false).unwrap();
    assert!(!report.reclustered);
    assert_eq!(index.face_assignments(other).unwrap()[0].person_id, person);
    let mut tiny = face(1, 0);
    tiny.bbox[2] = 4.;
    index.replace_faces(other, &[face(0, 0), tiny]).unwrap();
    assert!(job.run(&index, &[id, other], false).unwrap().reclustered);
    let assignments = index.face_assignments(other).unwrap();
    assert_ne!(assignments[1].person_id, person);
    index
        .assign_face(
            FaceKey {
                image_id: other,
                ordinal: 1,
            },
            &person,
        )
        .unwrap();
    index
        .confirm_face(
            FaceKey {
                image_id: other,
                ordinal: 1,
            },
            true,
        )
        .unwrap();
    assert!(job.run(&index, &[id, other], true).unwrap().reclustered);
    assert_eq!(index.face_assignments(other).unwrap()[1].person_id, person);
}

#[test]
fn suggestions_are_read_only_and_plan_failure_is_atomic() {
    let (_dir, index, id) = setup();
    let descriptor = face(0, 0).embedding.unwrap();
    index
        .create_person("named", Some("Anna"), Some(&descriptor))
        .unwrap();
    index
        .create_person("unknown", None, Some(&descriptor))
        .unwrap();
    let suggestions = ml_faces::people::name_suggestions(&index, 0.9).unwrap();
    assert_eq!(suggestions.len(), 1);
    assert_eq!(suggestions[0].named_id, "named");
    assert_eq!(suggestions[0].unnamed_id, "unknown");
    let before = index.people().unwrap();
    assert!(
        index
            .apply_people_plan(
                &[index::Person {
                    id: "new".into(),
                    name: None,
                    medoid: None
                }],
                &[(
                    FaceKey {
                        image_id: id,
                        ordinal: 999
                    },
                    "new".into()
                )]
            )
            .is_err()
    );
    assert_eq!(index.people().unwrap(), before);
}
