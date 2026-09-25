use super::*;

fn parse(s: &str) -> SavedSearch {
    s.parse().unwrap()
}
fn rule(field: &str, op: &str, value: Value) -> SavedSearch {
    SavedSearch::Rule {
        criteria: field.into(),
        operation: op.into(),
        value,
    }
}

#[test]
fn parses_and_compiles_native_rating() {
    let search = parse("rating>=3");
    assert_eq!(
        search.compile().unwrap().predicate,
        Some(Predicate::Grade(Comparison::Ge, 3.0))
    );
    assert_eq!(search.to_string().parse::<SavedSearch>().unwrap(), search);
    assert_eq!(
        parse("grade:0").compile().unwrap().predicate,
        Some(Predicate::Grade(Comparison::Eq, 0.0))
    );
}

#[test]
fn user_expression_roundtrip() {
    for text in [
        r#"rating>=3 keyword:beach camera:"Canon" date:2024-01..2024-06 lens:85mm focus>0.6 person:"Anna" semantic:"laughing""#,
        "cat OR dog AND NOT bird",
        "(cat OR dog) (bird OR fox)",
        r#"camera:"Canon \\" OR text:"a \"quote\"""#,
        "NOT NOT cat",
        "éclair\u{2003}keyword:海",
        "((cat AND dog) AND fox)",
    ] {
        let ast = parse(text);
        let canonical = ast.to_string();
        assert_eq!(parse(&canonical), ast, "{text} => {canonical}");
        assert_eq!(parse(&canonical).to_string(), canonical);
    }
}

#[test]
fn precedence_and_adjacency() {
    assert_eq!(
        parse("a OR b AND NOT c"),
        SavedSearch::Any(vec![
            parse("a"),
            SavedSearch::All(vec![parse("b"), SavedSearch::None(vec![parse("c")])])
        ])
    );
    assert_eq!(parse("a b"), parse("a AND b"));
    assert_eq!(parse("a and b or not c"), parse("a AND b OR NOT c"));
    assert!(matches!(parse("(a OR b) c"), SavedSearch::All(_)));
}

#[test]
fn malformed_input_has_byte_positions() {
    for input in [
        "",
        "()",
        "a AND",
        "OR a",
        "a OR OR b",
        "NOT",
        "(a",
        "a)",
        "keyword:",
        "rating=>3",
        "focus~~0.2:foo",
        "wat:x",
        "camera>Canon",
        "rating:4",
        "rating:1.2",
        "focus:-0.1",
        "focus:1.1",
        "focus:NaN",
        "focus:1e999",
        "decision:maybe",
        "keyword:\"\"",
        "\"unterminated",
        "\"\\q\"",
    ] {
        let err = input.parse::<SavedSearch>().unwrap_err().to_string();
        assert!(err.contains("byte"), "{input}: {err}");
    }
    assert!(
        "é AND )"
            .parse::<SavedSearch>()
            .unwrap_err()
            .to_string()
            .contains("byte 7")
    );
}

#[test]
fn typed_predicates_and_literal_fts() {
    let q = parse(
        r#"keyword:beach camera:"Canon" lens:85mm focus>0.6 person:"Anna" decision:keep mark:red"#,
    )
    .compile()
    .unwrap();
    assert_eq!(
        q.predicate,
        Some(Predicate::All(vec![
            Predicate::Keyword("beach".into()),
            Predicate::Camera("Canon".into()),
            Predicate::Lens("85mm".into()),
            Predicate::Focus(Comparison::Gt, 0.6),
            Predicate::Person("Anna".into()),
            Predicate::Decision("keep".into()),
            Predicate::Mark("red".into())
        ]))
    );
    assert!(q.text.is_none());
    assert_eq!(
        parse(r#""a \"quote\" OR *""#).compile().unwrap().predicate,
        Some(Predicate::Text("\"a \"\"quote\"\" OR *\"".into()))
    );
    assert_eq!(
        parse("camera!=Canon").compile().unwrap().predicate,
        Some(Predicate::Not(Box::new(Predicate::Camera("Canon".into()))))
    );
}

#[test]
fn dates_are_validated_and_use_exclusive_boundaries() {
    for (input, from, before) in [
        ("2024-01..2024-06", "2024-01-01", "2024-07-01"),
        ("2024-02-29", "2024-02-29", "2024-03-01"),
        ("2023-12-31", "2023-12-31", "2024-01-01"),
        ("2000-02", "2000-02-01", "2000-03-01"),
        ("2024", "2024-01-01", "2025-01-01"),
    ] {
        assert_eq!(
            parse(&format!("date:{input}")).compile().unwrap().predicate,
            Some(Predicate::All(vec![
                Predicate::DateFrom(from.into()),
                Predicate::DateBefore(before.into())
            ]))
        );
    }
    for input in [
        "2023-02-29",
        "1900-02-29",
        "2024-04-31",
        "2024-00",
        "2024-13",
        "0000",
        "2024-1",
        "2024-01-00",
        "2024-02..2024-01",
        "2024..",
        "..2024",
        "2024..2025..2026",
        "9999",
    ] {
        assert!(
            format!("date:{input}").parse::<SavedSearch>().is_err(),
            "{input}"
        );
    }
}

#[test]
fn semantic_is_only_a_single_positive_conjunct() {
    let q = parse(r#"semantic:"laughing" AND (camera:Canon OR camera:Nikon)"#)
        .compile()
        .unwrap();
    assert_eq!(q.semantic.as_deref(), Some("laughing"));
    assert!(q.predicate.is_some());
    for input in [
        "semantic:x OR camera:Canon",
        "NOT semantic:x",
        "NOT NOT semantic:x",
        "semantic:x semantic:y",
        "camera:Canon OR (keyword:x semantic:y)",
    ] {
        assert!(parse(input).compile().is_err(), "{input}");
    }
    assert_eq!(
        parse("semantic:x").compile().unwrap().semantic.as_deref(),
        Some("x")
    );
}

#[test]
fn arbitrary_imported_ast_and_empty_groups_roundtrip() {
    let unsupported = rule(
        "unknown Lightroom criterion",
        "contains",
        serde_json::json!({"original": [null, true, 5]}),
    );
    for ast in [
        SavedSearch::All(vec![]),
        SavedSearch::Any(vec![]),
        SavedSearch::None(vec![]),
        SavedSearch::All(vec![parse("cat")]),
        SavedSearch::None(vec![parse("cat"), parse("dog")]),
        unsupported.clone(),
        SavedSearch::All(vec![unsupported.clone(), SavedSearch::Any(vec![])]),
    ] {
        assert_eq!(parse(&ast.to_string()), ast);
        assert_eq!(
            serde_json::from_str::<SavedSearch>(&serde_json::to_string(&ast).unwrap()).unwrap(),
            ast
        );
    }
    assert!(unsupported.compile().is_err());
    for ast in [
        rule("rating", ">=", serde_json::json!(5)),
        rule("rating", "contains", serde_json::json!(2)),
        rule("camera", "contains", serde_json::json!("Canon")),
        rule("focus", ">", Value::Null),
    ] {
        assert!(ast.compile().is_err());
        assert_eq!(parse(&ast.to_string()), ast);
    }
    assert_eq!(
        SavedSearch::All(vec![]).compile().unwrap().predicate,
        Some(Predicate::All(vec![]))
    );
    assert_eq!(
        SavedSearch::Any(vec![]).compile().unwrap().predicate,
        Some(Predicate::Any(vec![]))
    );
    assert_eq!(
        SavedSearch::None(vec![]).compile().unwrap().predicate,
        Some(Predicate::Not(Box::new(Predicate::Any(vec![]))))
    );
}

#[test]
fn encoded_ast_validation_and_original_shape() {
    fn envelope(json: &str) -> String {
        format!("@json:{}", serde_json::to_string(json).unwrap())
    }
    let ast = SavedSearch::All(vec![rule("unsupported", "exists", Value::Bool(true))]);
    assert_eq!(parse(&envelope(&serde_json::to_string(&ast).unwrap())), ast);
    for json in [
        "[]",
        r#"[{"All":1}]"#,
        r#"[{"Any":0},{"None":0}]"#,
        r#"[{"All":18446744073709551615}]"#,
        r#"[{"Other":0}]"#,
        "null",
    ] {
        assert!(envelope(json).parse::<SavedSearch>().is_err(), "{json}");
    }
    assert!("@json:not_json".parse::<SavedSearch>().is_err());
    let encoded =
        serde_json::to_string(&flatten(&SavedSearch::All(vec![parse("a"); MAX_NODES]))).unwrap();
    assert!(envelope(&encoded).parse::<SavedSearch>().is_err());
}

#[test]
fn every_numeric_comparison_is_compiled() {
    for (op, expected) in [
        (":", Comparison::Eq),
        ("=", Comparison::Eq),
        ("==", Comparison::Eq),
        ("!=", Comparison::Ne),
        ("<", Comparison::Lt),
        ("<=", Comparison::Le),
        (">", Comparison::Gt),
        (">=", Comparison::Ge),
    ] {
        assert_eq!(
            parse(&format!("focus{op}0.5")).compile().unwrap().predicate,
            Some(Predicate::Focus(expected, 0.5))
        );
    }
}

#[test]
fn boundary_sized_searches_display_roundtrip() {
    let ast = SavedSearch::All(vec![parse(&"x".repeat(MAX_BYTES - 7))]);
    assert_eq!(parse(&ast.to_string()), ast);
    let mut ast = parse("x");
    for _ in 0..MAX_DEPTH - 1 {
        ast = SavedSearch::All(vec![ast]);
    }
    assert_eq!(parse(&ast.to_string()), ast);
}

#[test]
fn bounded_parsing_and_compilation() {
    assert!("a".repeat(MAX_BYTES + 1).parse::<SavedSearch>().is_err());
    assert!(
        format!("{}a{}", "(".repeat(MAX_DEPTH), ")".repeat(MAX_DEPTH))
            .parse::<SavedSearch>()
            .is_err()
    );
    assert!("NOT ".repeat(MAX_DEPTH).parse::<SavedSearch>().is_err());
    assert!("a ".repeat(MAX_NODES + 1).parse::<SavedSearch>().is_err());
    let mut ast = parse("a");
    for _ in 0..MAX_DEPTH {
        ast = SavedSearch::All(vec![ast]);
    }
    assert!(ast.compile().is_err());
    assert!(
        SavedSearch::All(vec![parse("a"); MAX_NODES + 1])
            .compile()
            .is_err()
    );
}
