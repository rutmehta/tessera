//! Derive schema mirrors from engine-api's actual serde declarations, without
//! introducing a schema dependency into the engine contract. Only wire IDs need
//! substitution: their custom serializers emit strings or unsigned integers.
use quote::{ToTokens, quote};
use std::{env, fs, path::PathBuf};
use syn::{Item, parse_quote};

fn main() {
    let root = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap()).join("../engine-api/src");
    let mut output = quote! {
        use serde::{Serialize,Deserialize};
        type ImageId=String; type RecipeHash=String; type IccProfileHandle=String;
        type MaskId=u64; type PersonId=u64; type HistoryGroupId=u64;
        type StyleId=String; type ModelId=String; type Grade=u8; type Mark=String;
    };
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
                "Resize",
                "ExportSettings",
            ],
        ),
        (
            "recipe/mask.rs",
            vec![
                "PersonPart",
                "LandscapeClass",
                "MaskKind",
                "BrushStroke",
                "MaskCombine",
                "MaskComponent",
                "LocalParams",
            ],
        ),
        ("recipe/settings.rs", vec!["NormalizedRect"]),
        ("recipe/selection.rs", vec!["Decision"]),
        ("id.rs", vec!["ModelRef"]),
    ];
    let mut tool_structs = Vec::new();
    for (file, names) in sources {
        let path = root.join(file);
        println!("cargo:rerun-if-changed={}", path.display());
        let parsed = syn::parse_file(&fs::read_to_string(path).unwrap()).unwrap();
        let envelope = parsed
            .items
            .iter()
            .find_map(|item| match item {
                Item::Struct(s) if s.ident == "ToolRequest" => Some(
                    s.fields
                        .iter()
                        .filter(|f| f.ident.as_ref().is_some_and(|i| i != "call"))
                        .cloned()
                        .collect::<Vec<_>>(),
                ),
                _ => None,
            })
            .unwrap_or_default();
        for item in parsed.items {
            match item {
                Item::Struct(mut s) if names.iter().any(|name| s.ident == name) => {
                    s.attrs.push(parse_quote!(#[derive(schemars::JsonSchema)]));
                    output.extend(s.into_token_stream());
                }
                Item::Enum(mut e) if names.iter().any(|name| e.ident == name) => {
                    e.attrs.push(parse_quote!(#[derive(schemars::JsonSchema)]));
                    output.extend(e.into_token_stream());
                }
                Item::Enum(e) if e.ident == "ToolCall" => {
                    for variant in e.variants {
                        let name = variant.ident;
                        let syn::Fields::Named(fields) = variant.fields else {
                            panic!("tool variants must have named fields");
                        };
                        let fields = fields.named.iter();
                        output.extend(quote! {
                            #[derive(Debug,Clone,Serialize,Deserialize,schemars::JsonSchema)]
                            #[serde(deny_unknown_fields)]
                            pub struct #name { #(#fields,)* #(#envelope,)* }
                        });
                        tool_structs.push(name);
                    }
                }
                Item::Impl(i)
                    if i.self_ty.to_token_stream().to_string() == "BrushStroke"
                        && i.trait_.is_some() =>
                {
                    output.extend(i.into_token_stream())
                }
                Item::Impl(i) if i.self_ty.to_token_stream().to_string() == "NormalizedRect" => {
                    output.extend(i.into_token_stream())
                }
                Item::Fn(f)
                    if file == "tools.rs"
                        && ["default_template", "yes", "hundred", "default_bins"]
                            .iter()
                            .any(|name| f.sig.ident == name) =>
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
    });
    fs::write(
        PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("wire_schemas.rs"),
        output.to_string(),
    )
    .unwrap();
}
