//! Derive schema mirrors from engine-api's actual serde declarations, without
//! introducing a schema dependency into the engine contract. Only wire IDs need
//! substitution: their custom serializers emit strings or unsigned integers.
//!
//! Both call enums are mirrored: every `ToolCall` variant becomes a struct with
//! the `ToolRequest` envelope, and every `DocumentToolCall` variant a struct
//! with the `DocumentToolRequest` envelope (`rationale`, `group`,
//! `expect_head`).
use quote::{ToTokens, quote};
use std::{env, fs, path::PathBuf};
use syn::{Field, Item, parse_quote};

fn envelope(items: &[Item], name: &str) -> Vec<Field> {
    items
        .iter()
        .find_map(|item| match item {
            Item::Struct(s) if s.ident == name => Some(
                s.fields
                    .iter()
                    .filter(|f| f.ident.as_ref().is_some_and(|i| i != "call"))
                    .cloned()
                    .collect::<Vec<_>>(),
            ),
            _ => None,
        })
        .unwrap_or_else(|| panic!("engine-api has no {name}"))
}

// Schema-only mirrors preserve the model serde declarations without pulling
// schema dependencies into typography or vector.
fn model_mirrors(
    root: &std::path::Path,
    crate_name: &str,
    files: &[&str],
) -> proc_macro2::TokenStream {
    let mut out = quote! {};
    for file in files {
        let path = root.join(format!("../../{crate_name}/src/{file}"));
        println!("cargo:rerun-if-changed={}", path.display());
        let source = fs::read_to_string(path)
            .unwrap()
            .replace("crate::PathText", "PathText");
        for item in syn::parse_file(&source).unwrap().items {
            match item {
                Item::Struct(mut s)
                    if matches!(s.vis, syn::Visibility::Public(_))
                        && s.attrs
                            .iter()
                            .any(|a| a.to_token_stream().to_string().contains("Serialize")) =>
                {
                    s.attrs.push(parse_quote!(#[derive(schemars::JsonSchema)]));
                    out.extend(s.into_token_stream());
                }
                Item::Enum(mut e)
                    if matches!(e.vis, syn::Visibility::Public(_))
                        && e.attrs
                            .iter()
                            .any(|a| a.to_token_stream().to_string().contains("Serialize")) =>
                {
                    e.attrs.push(parse_quote!(#[derive(schemars::JsonSchema)]));
                    out.extend(e.into_token_stream());
                }
                Item::Impl(i)
                    if i.trait_
                        .as_ref()
                        .is_some_and(|(_, p, _)| p.is_ident("Default")) =>
                {
                    out.extend(i.into_token_stream())
                }
                _ => {}
            }
        }
    }
    out
}

fn main() {
    let root = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap()).join("../engine-api/src");
    let text = model_mirrors(&root, "typography", &["model.rs", "path_data.rs"]);
    let vector = model_mirrors(
        &root,
        "vector",
        &["lib.rs", "geometry.rs", "fill.rs", "stroke.rs"],
    );
    let mut output = quote! {
        mod vector {
            use serde::{Serialize, Deserialize};
            type Color = [f32; 4];
            type Affine = [f64; 6];
            #[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
            pub struct Point { pub x: f64, pub y: f64 }
            type Vec2 = Point;
            #[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
            pub struct Rect { pub x0: f64, pub y0: f64, pub x1: f64, pub y1: f64 }
            #[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
            pub enum LineCap { Butt, Square, Round }
            #[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
            pub enum LineJoin { Miter, MiterClip, Round, Bevel }
            #vector
        }
        use vector::ShapeModel;

        mod typography {
            use serde::{Serialize, Deserialize};
            use std::collections::BTreeMap;
            #text
        }
        use typography::{TextModel,TextRun};

        use serde::{Serialize,Deserialize};
        type ImageId=String; type RecipeHash=String; type IccProfileHandle=String;
        type MaskId=u64; type PersonId=u64; type HistoryGroupId=u64;
        type StyleId=String; type ModelId=String; type Grade=u8; type Mark=String;
        type DocumentId=u64; type LayerId=u64; type SelectionId=u64; type HistoryEntryId=u64;
        type ChannelId=u64; type Digest=String;
    };
    let tools_path = root.join("tools.rs");
    let tools_items = syn::parse_file(&fs::read_to_string(&tools_path).unwrap())
        .unwrap()
        .items;
    let recipe_envelope = envelope(&tools_items, "ToolRequest");
    let document_envelope = envelope(&tools_items, "DocumentToolRequest");
    let library_items =
        syn::parse_file(&fs::read_to_string(root.join("tools/library.rs")).unwrap())
            .unwrap()
            .items;
    let library_envelope = envelope(&library_items, "LibraryToolRequest");
    let sources = [
        (
            "tools.rs",
            vec![
                "ToneUpdate",
                "RemovalMethod",
                "FieldUpdate",
                "HistogramSpace",
                "CompareMetric",
                "ExportFormat",
                "ExportMetadata",
                "HdrTransfer",
                "Resize",
                "ExportSettings",
            ],
        ),
        (
            "document.rs",
            vec![
                "BlendMode",
                "GroupMode",
                "CanvasRect",
                "NewLayer",
                "LayerPropsUpdate",
                "StrokePoint",
                "BrushMode",
                "BrushParams",
                "StrokeTarget",
                "SelectionShape",
                "SelectionMode",
                "LevelsChannel",
                "AdjustmentSpec",
                "Interpolation",
                "AffineTransform",
                "DocumentFormat",
                "DocumentExportSettings",
                "DocumentDepth",
                "ChannelKind",
                "ChannelRasterRef",
                "ChannelSummary",
                "VectorMask",
            ],
        ),
        (
            "recipe/mask.rs",
            vec![
                "PersonPart",
                "LandscapeClass",
                "MaskKind",
                "LuminanceDomain",
                "BrushStroke",
                "MaskCombine",
                "MaskComponent",
                "LocalParams",
            ],
        ),
        ("recipe/settings.rs", vec!["NormalizedRect"]),
        ("recipe/selection.rs", vec!["Decision"]),
        ("id.rs", vec!["ModelRef"]),
        ("tile.rs", vec!["Extent"]),
        (
            "people.rs",
            vec![
                "FaceRef",
                "PersonSummary",
                "FaceRegion",
                "NameSuggestion",
                "QualityGate",
                "ClusterOptions",
                "PeopleJobResult",
                "PeopleWriteOptions",
            ],
        ),
        ("tools/library.rs", vec![]),
    ];
    let mut tool_structs = Vec::new();
    let mut document_structs = Vec::new();
    let mut library_structs = Vec::new();
    // serde rejects every key of a flattened *enum* under
    // `deny_unknown_fields`, so those mirrors omit it; `schema::validate`
    // then checks keys against the derived schema instead.
    let mut enums = Vec::new();
    for (file, _) in &sources {
        let parsed = syn::parse_file(&fs::read_to_string(root.join(file)).unwrap()).unwrap();
        for item in parsed.items {
            if let Item::Enum(e) = item {
                enums.push(e.ident.to_string());
            }
        }
    }
    let flattens_enum = |fields: &syn::FieldsNamed| {
        fields.named.iter().any(|f| {
            f.attrs.iter().any(|a| {
                a.path().is_ident("serde") && a.to_token_stream().to_string().contains("flatten")
            }) && matches!(&f.ty, syn::Type::Path(p) if p.path.segments.last().is_some_and(|s| enums.contains(&s.ident.to_string())))
        })
    };
    for (file, names) in sources {
        let path = root.join(file);
        println!("cargo:rerun-if-changed={}", path.display());
        let parsed = syn::parse_file(&fs::read_to_string(path).unwrap()).unwrap();
        for item in parsed.items {
            match item {
                Item::Struct(mut s) if names.iter().any(|name| s.ident == name) => {
                    // ImageId is Copy in engine-api but a String in schema mirrors.
                    if s.ident == "FaceRef" {
                        s.attrs.retain(|a| !a.path().is_ident("derive"));
                        s.attrs.push(parse_quote!(#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]));
                    }
                    s.attrs.push(parse_quote!(#[derive(schemars::JsonSchema)]));
                    output.extend(s.into_token_stream());
                }
                Item::Enum(mut e) if names.iter().any(|name| e.ident == name) => {
                    e.attrs.push(parse_quote!(#[derive(schemars::JsonSchema)]));
                    output.extend(e.into_token_stream());
                }
                Item::Enum(e)
                    if e.ident == "ToolCall"
                        || e.ident == "DocumentToolCall"
                        || e.ident == "LibraryToolCall" =>
                {
                    let document = e.ident == "DocumentToolCall";
                    let library = e.ident == "LibraryToolCall";
                    let envelope = if document {
                        &document_envelope
                    } else if library {
                        &library_envelope
                    } else {
                        &recipe_envelope
                    };
                    for variant in e.variants {
                        let name = variant.ident;
                        let docs = variant.attrs.iter().filter(|a| a.path().is_ident("doc"));
                        let syn::Fields::Named(fields) = variant.fields else {
                            panic!("tool variants must have named fields");
                        };
                        let deny = (!flattens_enum(&fields))
                            .then(|| quote!(#[serde(deny_unknown_fields)]));
                        let fields = fields.named.iter();
                        output.extend(quote! {
                            #(#docs)*
                            #[derive(Debug,Clone,Serialize,Deserialize,schemars::JsonSchema)]
                            #deny
                            pub struct #name { #(#fields,)* #(#envelope,)* }
                        });
                        if document {
                            document_structs.push(name);
                        } else if library {
                            library_structs.push(name);
                        } else {
                            tool_structs.push(name);
                        }
                    }
                }
                Item::Impl(i)
                    if i.self_ty.to_token_stream().to_string() == "BrushStroke"
                        && i.trait_.is_some() =>
                {
                    output.extend(i.into_token_stream())
                }
                Item::Impl(i)
                    if ["NormalizedRect", "LuminanceDomain"]
                        .contains(&i.self_ty.to_token_stream().to_string().as_str()) =>
                {
                    output.extend(i.into_token_stream())
                }
                Item::Impl(i)
                    if (file == "document.rs" || file == "people.rs")
                        && names
                            .iter()
                            .any(|name| i.self_ty.to_token_stream().to_string() == *name) =>
                {
                    output.extend(i.into_token_stream())
                }
                Item::Fn(f)
                    if file == "tools.rs"
                        && [
                            "default_template",
                            "yes",
                            "hundred",
                            "default_bins",
                            "present",
                        ]
                        .iter()
                        .any(|name| f.sig.ident == name) =>
                {
                    output.extend(f.into_token_stream())
                }
                Item::Fn(f) if file == "recipe/mask.rs" && f.sig.ident == "is_true" => {
                    output.extend(f.into_token_stream())
                }
                // `yes` is already emitted from tools.rs (same body).
                Item::Fn(f)
                    if file == "document.rs"
                        && ["one", "identity3"].iter().any(|name| f.sig.ident == name) =>
                {
                    output.extend(f.into_token_stream())
                }
                _ => {}
            }
        }
    }
    output.extend(quote! {
        pub fn engine_schemas()->Vec<serde_json::Value> {
            vec![#(serde_json::to_value(schemars::schema_for!(#tool_structs)).expect("serializable schema")),*]
        }
        pub fn validate_engine(index:usize,value:serde_json::Value)->Result<(),serde_json::Error> {
            type Parser=fn(serde_json::Value)->Result<(),serde_json::Error>;
            let parsers:&[Parser]=&[#(|v|serde_json::from_value::<#tool_structs>(v).map(|_|())),*];
            parsers[index](value)
        }
        pub fn document_schemas()->Vec<serde_json::Value> {
            vec![#(serde_json::to_value(schemars::schema_for!(#document_structs)).expect("serializable schema")),*]
        }
        pub fn library_schemas()->Vec<serde_json::Value> {
            vec![#(serde_json::to_value(schemars::schema_for!(#library_structs)).expect("serializable schema")),*]
        }
        pub fn validate_library(index:usize,value:serde_json::Value)->Result<(),serde_json::Error> {
            type Parser=fn(serde_json::Value)->Result<(),serde_json::Error>;
            let parsers:&[Parser]=&[#(|v|serde_json::from_value::<#library_structs>(v).map(|_|())),*];
            parsers[index](value)
        }
        pub fn validate_document(index:usize,value:serde_json::Value)->Result<(),serde_json::Error> {
            type Parser=fn(serde_json::Value)->Result<(),serde_json::Error>;
            let parsers:&[Parser]=&[#(|v|serde_json::from_value::<#document_structs>(v).map(|_|())),*];
            parsers[index](value)
        }
    });
    fs::write(
        PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("wire_schemas.rs"),
        output.to_string(),
    )
    .unwrap();
}
