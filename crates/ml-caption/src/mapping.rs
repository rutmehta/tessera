use anyhow::{Result, ensure};
use engine_api::id::ImageId;
use library::{Keyword, Library};
use serde::{Deserialize, Serialize};
use sidecar::{MarkPreset, Sidecar, XmpPacket};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MappedKeyword {
    pub path: Vec<String>,
    pub proposed: bool,
}
/// Mapping never edits the user's tree. Exact normalized names take precedence
/// over synonyms. Ambiguous aliases are rejected rather than silently mis-tagged.
pub fn map_keyword(library: &Library, label: &str) -> Result<MappedKeyword> {
    let label = label.trim();
    ensure!(
        !label.is_empty() && !label.contains('|') && !label.chars().any(char::is_control),
        "invalid keyword name"
    );
    fn walk(
        nodes: &[Keyword],
        prefix: &mut Vec<String>,
        query: &str,
        names: &mut Vec<Vec<String>>,
        aliases: &mut Vec<Vec<String>>,
    ) {
        for node in nodes {
            prefix.push(node.name.clone());
            if node.name.trim().to_lowercase() == query {
                names.push(prefix.clone());
            } else if node
                .synonyms
                .iter()
                .any(|s| s.trim().to_lowercase() == query)
            {
                aliases.push(prefix.clone());
            }
            walk(&node.children, prefix, query, names, aliases);
            prefix.pop();
        }
    }
    let (mut names, mut aliases) = (Vec::new(), Vec::new());
    walk(
        &library.keywords,
        &mut Vec::new(),
        &label.to_lowercase(),
        &mut names,
        &mut aliases,
    );
    let matches = if names.is_empty() { aliases } else { names };
    ensure!(matches.len() <= 1, "ambiguous keyword or synonym: {label}");
    match matches.into_iter().next() {
        Some(path) => Ok(MappedKeyword {
            path,
            proposed: false,
        }),
        None => {
            let mut path = library
                .keyword_pairs()
                .iter()
                .find(|(name, _)| name.trim().eq_ignore_ascii_case("Suggested"))
                .and_then(|(name, _)| library.keyword_path(name))
                .unwrap_or_else(|| vec!["Suggested".into()]);
            path.push(label.into());
            Ok(MappedKeyword {
                path,
                proposed: true,
            })
        }
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum WritePolicy {
    #[default]
    IndexOnly,
    Xmp,
}

/// Explicit bulk acceptance. Suggestions themselves never call this function.
/// Preflights all names/images/XMP before writes. File/index/library changes
/// cannot form a cross-resource transaction; errors propagate and retry is
/// idempotent. Caller persists the returned in-memory library using Library::write.
pub fn accept_keywords(
    index: &index::Index,
    library: &mut Library,
    images: &[ImageId],
    labels: &[String],
    policy: WritePolicy,
) -> Result<Vec<MappedKeyword>> {
    let mapped = labels
        .iter()
        .map(|s| map_keyword(library, s))
        .collect::<Result<Vec<_>>>()?;
    let mut next = library.clone();
    for mapping in &mapped {
        let mut parent: Option<&str> = None;
        for name in &mapping.path {
            if next.keyword_path(name).is_none() {
                next.add_keyword(name, parent)?;
            }
            parent = Some(name);
        }
    }
    let mut packets = Vec::new();
    for &id in images {
        let info = index.image_info(id)?;
        if policy == WritePolicy::Xmp {
            let path = Sidecar::paths(&info.path).xmp;
            let packet = if path.exists() {
                Sidecar::read_xmp(&path)?
            } else {
                XmpPacket::from_selection(
                    &index.selection(id)?.unwrap_or_default(),
                    &MarkPreset::lightroom(),
                )
            };
            let mut metadata = packet.metadata()?;
            for mapping in &mapped {
                let name = mapping.path.last().unwrap();
                if !metadata.keywords.contains(name) {
                    metadata.keywords.push(name.clone());
                }
                let path = mapping.path.join("|");
                if !metadata.hierarchical_keywords.contains(&path) {
                    metadata.hierarchical_keywords.push(path);
                }
            }
            // Preserve the existing XMP selection, not a potentially stale index.
            let updated =
                packet.with_metadata(&packet.selection()?, &metadata, &MarkPreset::lightroom())?;
            packets.push((path, updated));
        }
    }
    index.sync_keyword_tree(&next.keyword_pairs())?;
    for (path, packet) in packets {
        Sidecar::write_xmp(path, &packet)?;
    }
    let names = mapped
        .iter()
        .map(|m| m.path.last().unwrap().clone())
        .collect::<Vec<_>>();
    index.accept_keyword_names(images, &names)?;
    *library = next;
    Ok(mapped)
}
