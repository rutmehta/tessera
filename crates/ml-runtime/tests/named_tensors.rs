use ml_runtime::{Session, SessionOptions, TensorInput, TensorOutput};

fn session(name: &str) -> anyhow::Result<Session> {
    Session::load(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/data")
            .join(name),
        SessionOptions::cpu(),
    )
}

#[test]
fn dimension_overrides_pin_symbolic_batch_without_changing_default_load() -> anyhow::Result<()> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/tokens.onnx");
    let mut pinned = Session::load_with_dimensions(&path, SessionOptions::cpu(), &[("batch", 2)])?;
    let inputs = |batch| {
        ["input_ids", "attention_mask"].map(|name| {
            (
                name,
                TensorInput::I64 {
                    shape: vec![batch, 3],
                    data: vec![1; batch * 3],
                },
            )
        })
    };
    assert_eq!(pinned.run_tensors(&inputs(2))?[0].shape, [2, 3]);
    assert!(
        pinned.run_tensors(&inputs(1)).is_err(),
        "pinned batch must reject a different size"
    );
    let mut dynamic = Session::load(&path, SessionOptions::cpu())?;
    assert_eq!(dynamic.run_tensors(&inputs(1))?[0].shape, [1, 3]);
    assert_eq!(dynamic.run_tensors(&inputs(3))?[0].shape, [3, 3]);
    Ok(())
}

#[test]
fn fp16_inputs_and_outputs_preserve_arbitrary_rank_and_batches() -> anyhow::Result<()> {
    for rank in [0, 1, 2, 3, 5] {
        let shape = vec![2; rank];
        let data: Vec<f32> = (0..(1 << rank)).map(|i| i as f32 / 3. - 0.5).collect();
        let mut model = session(&format!("identity-rank{rank}-fp16.onnx"))?;
        let outputs = model.run_tensors(&[(
            "input",
            TensorInput::F32 {
                shape: shape.clone(),
                data: data.clone(),
            },
        )])?;
        assert_eq!(outputs[0].shape, shape);
        assert_eq!(
            outputs[0].data,
            data.iter()
                .map(|&v| half::f16::from_f32(v).to_f32())
                .collect::<Vec<_>>()
        );
    }
    Ok(())
}

#[test]
fn named_i64_tokens_return_multiple_float_outputs() -> anyhow::Result<()> {
    let mut model = session("tokens.onnx")?;
    // Reverse input order to verify binding by name, not position.
    let outputs = model.run_tensors(&[
        (
            "attention_mask",
            TensorInput::I64 {
                shape: vec![2, 3],
                data: vec![1, 0, 1, 0, 1, 0],
            },
        ),
        (
            "input_ids",
            TensorInput::I64 {
                shape: vec![2, 3],
                data: vec![2, 4, 6, 8, 10, 12],
            },
        ),
    ])?;
    assert_eq!(outputs.len(), 2);
    assert_eq!(outputs[0].name, "embeddings");
    assert_eq!(outputs[0].shape, [2, 3]);
    assert_eq!(outputs[0].data, [1., 4., 5., 8., 9., 12.]);
    assert_eq!(outputs[1].name, "half_embeddings");
    assert_eq!(outputs[1].shape, [2, 3]);
    assert_eq!(outputs[1].data, [2., 4., 6., 8., 10., 12.]);
    assert!(!model.partition_report()?.nodes.is_empty());
    Ok(())
}

#[test]
fn int64_manifest_specs_support_partition_probes() -> anyhow::Result<()> {
    let dtype: ml_runtime::Dtype = serde_json::from_str("\"int64\"")?;
    assert_eq!(dtype, ml_runtime::Dtype::Int64);
    let mut model = session("tokens.onnx")?;
    model.probe(
        &["input_ids", "attention_mask"].map(|name| ml_runtime::TensorSpec {
            name: name.into(),
            shape: vec![2, 3],
            dtype,
        }),
    )?;
    assert!(!model.partition_report()?.nodes.is_empty());
    Ok(())
}

#[test]
fn invalid_inputs_return_errors_without_panicking() -> anyhow::Result<()> {
    let mut model = session("identity-rank2-fp32.onnx")?;
    for shape in [vec![usize::MAX, 2], vec![usize::MAX], vec![2, 2]] {
        assert!(
            model
                .run_tensors(&[(
                    "input",
                    TensorInput::F32 {
                        shape,
                        data: vec![]
                    }
                )])
                .is_err()
        );
    }
    let input = TensorInput::F32 {
        shape: vec![2, 2],
        data: vec![0.; 4],
    };
    assert!(model.run_tensors(&[]).is_err());
    assert!(model.run_tensors(&[("unknown", input.clone())]).is_err());
    assert!(
        model
            .run_tensors(&[("input", input.clone()), ("input", input.clone())])
            .is_err()
    );
    assert!(
        model
            .run_tensors(&[(
                "input",
                TensorInput::I64 {
                    shape: vec![2, 2],
                    data: vec![0; 4]
                }
            )])
            .is_err()
    );
    assert!(model.run_tensors(&[("input", input)]).is_ok());
    Ok(())
}

#[test]
fn fp32_outputs_preserve_arbitrary_rank_and_batches() -> anyhow::Result<()> {
    for rank in [0, 1, 2, 3, 5] {
        let shape = vec![2; rank];
        let data: Vec<f32> = (0..(1 << rank)).map(|i| i as f32 - 0.5).collect();
        let mut model = session(&format!("identity-rank{rank}-fp32.onnx"))?;
        let outputs: Vec<TensorOutput> = model.run_tensors(&[(
            "input",
            TensorInput::F32 {
                shape: shape.clone(),
                data: data.clone(),
            },
        )])?;
        assert_eq!(outputs.len(), 1);
        assert_eq!(outputs[0].name, "output");
        assert_eq!(outputs[0].shape, shape);
        assert_eq!(outputs[0].data, data);
    }
    Ok(())
}
