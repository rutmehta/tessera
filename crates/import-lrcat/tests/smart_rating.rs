mod make_fixture;
use engine_api::recipe::{Decision, Grade, Selection};
use import_lrcat::{SavedSearch, import};

// Evaluate the translated AST against the same selection fields compiled by
// library::SavedSearch. NULL grade never satisfies a numeric comparison.
fn matches(search: &SavedSearch, selection: &Selection) -> bool {
    match search {
        SavedSearch::All(v) => v.iter().all(|s| matches(s, selection)),
        SavedSearch::Any(v) => v.iter().any(|s| matches(s, selection)),
        SavedSearch::None(v) => !v.iter().any(|s| matches(s, selection)),
        SavedSearch::Rule {
            criteria,
            operation,
            value,
        } => match criteria.as_str() {
            "decision" => {
                operation == ":"
                    && value.as_str()
                        == Some(match selection.decision {
                            Decision::Keep => "keep",
                            Decision::Reject => "reject",
                            Decision::Undecided => "undecided",
                        })
            }
            "grade" => {
                let grade = match selection.grade {
                    Some(Grade::One) => 1,
                    Some(Grade::Two) => 2,
                    Some(Grade::Three) => 3,
                    None => return false,
                };
                let threshold = value.as_i64().unwrap();
                match operation.as_str() {
                    "=" => grade == threshold,
                    ">=" => grade >= threshold,
                    _ => panic!("unexpected grade operator {operation}"),
                }
            }
            _ => panic!("untranslated criterion {criteria}"),
        },
    }
}

fn equivalent(a: i64, b: i64) -> bool {
    a == b || (a == 3 && b == 4) || (a == 4 && b == 3)
}

#[test]
fn all_star_thresholds_operators_ranges_and_flags() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("rating.lrcat");
    let db = make_fixture::make_fixture(&path);
    // Make all six star categories distinct rows, in addition to an explicitly
    // picked unrated row and a rejected row from the base fixture.
    for stars in 0..=5 {
        db.execute(
            "INSERT INTO Adobe_images VALUES(?1,20,NULL,NULL,'AB','2026-01-01',0,?2,'')",
            [40 + stars, stars],
        )
        .unwrap();
    }
    db.execute(
        "INSERT INTO Adobe_images VALUES(50,20,NULL,NULL,'AB','2026-01-01',1,0,'')",
        [],
    )
    .unwrap();
    for op in [">=", ">", "=", "<=", "<", "!="] {
        for threshold in 0..=5 {
            let raw = format!("{{criteria='rating',operation='{op}',value={threshold}}}");
            db.execute(
                "UPDATE AgLibraryCollectionContent SET content=?1 WHERE collection=3",
                [&raw],
            )
            .unwrap();
            let plan = import(&path).unwrap();
            let search = &plan.library.smart_albums[0].search;
            search.compile().unwrap();
            for image in plan
                .images
                .iter()
                .filter(|i| (40..=45).contains(&i.catalog_id))
            {
                let actual = matches(search, &image.selection);
                let star = image.rating.unwrap();
                let expected = (0..=5).any(|n| {
                    let qualifies = match op {
                        ">=" => n >= threshold,
                        ">" => n > threshold,
                        "=" => n == threshold,
                        "<=" => n <= threshold,
                        "<" => n < threshold,
                        "!=" => n != threshold,
                        _ => unreachable!(),
                    };
                    qualifies && equivalent(star, n)
                });
                assert_eq!(actual, expected, "{raw} on {star} stars");
            }
            if op == "=" && threshold == 0 {
                assert!(!matches(
                    search,
                    &plan
                        .images
                        .iter()
                        .find(|i| i.catalog_id == 50)
                        .unwrap()
                        .selection
                ));
            }
        }
    }
    for (value, expected) in [("2..4", &[2, 3, 4][..]), ("3..3", &[3, 4][..])] {
        let raw = format!("{{criteria='rating',operation='between',value='{value}'}}");
        db.execute(
            "UPDATE AgLibraryCollectionContent SET content=?1 WHERE collection=3",
            [&raw],
        )
        .unwrap();
        let plan = import(&path).unwrap();
        let search = &plan.library.smart_albums[0].search;
        search.compile().unwrap();
        for image in plan
            .images
            .iter()
            .filter(|i| (40..=45).contains(&i.catalog_id))
        {
            assert_eq!(
                matches(search, &image.selection),
                expected.contains(&image.rating.unwrap()),
                "{raw}"
            );
        }
    }
    for (criteria, value, target) in [
        ("pick", "1", "keep"),
        ("pick", "-1", "reject"),
        ("reject", "1", "reject"),
    ] {
        let raw = format!("{{criteria='{criteria}',operation='=',value={value}}}");
        db.execute(
            "UPDATE AgLibraryCollectionContent SET content=?1 WHERE collection=3",
            [&raw],
        )
        .unwrap();
        let plan = import(&path).unwrap();
        let search = &plan.library.smart_albums[0].search;
        search.compile().unwrap();
        assert!(matches(
            search,
            &plan
                .images
                .iter()
                .find(|i| match target {
                    "keep" => i.catalog_id == 50,
                    _ => i.catalog_id == 31,
                })
                .unwrap()
                .selection
        ));
    }
    let raw = "{criteria='rating',operation='=',value=6}";
    db.execute(
        "UPDATE AgLibraryCollectionContent SET content=?1 WHERE collection=3",
        [raw],
    )
    .unwrap();
    assert!(
        import(&path).unwrap().library.smart_albums[0]
            .search
            .compile()
            .is_err()
    );
}
