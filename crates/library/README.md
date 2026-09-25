# Library engine

`library` owns the single `Library`, `Album`, `AlbumGroup`, `SmartAlbum`,
`Keyword`, and `SavedSearch` definitions. `import-lrcat` re-exports the types;
`cull` re-exports `Library` and `Album` for existing host callers. No engine-api
changes or dependency on ml-embed are required.

## Document and operations

`Library::read` reads library.json (missing files yield an empty library).
`write` uses a same-directory temporary file, fsync, rename, and directory fsync.
Malformed documents and unsupported schema versions fail instead of resetting.
Top-level and album unknown fields are retained. Roots, people names, keyword
trees, marks presets and an opaque publish-state map are persisted alongside
manual albums, groups and smart albums.

Albums are a map from basket/UI handle to an album containing a stable integer
ID, display name, optional parent group, and ordered `ImageId` list. This retains
the cull host's map API. Imported duplicate names receive disambiguated handles.
Imported members use the exact identities of the corresponding imported recipes,
not Lightroom-local integer IDs. The former importer-only array document is not
a supported on-disk input format; re-import its source catalog to regenerate it.

`create_album`, `add_to_album`, `remove_from_album`, `reorder_album`,
`rename_album`, and `delete_album` operate only on the document. Removal/deletion
never opens or deletes original photos or sidecars. Reordering requires an exact
permutation. `in_album` supplies the derived-status hook used by cull. Cull's
basket still uses its global undo stack and rejects conflicting album changes.

`smart_query(id)` compiles a saved search. A smart album with a parent searches
only manual memberships in that group and all nested groups. Empty groups match
nothing. Cycles, duplicate group IDs and missing group ancestors are errors,
never a reason to fall back to an unscoped search. No parent means global scope.

## Saved-search text

Parse with `text.parse::<SavedSearch>()`, display with `to_string()`, compile with
`compile()`. Example:

    rating>=3 AND (keyword:beach OR camera:"Canon") NOT decision:reject date:2024-01..2024-06 lens:85mm focus>0.6 person:"Anna" semantic:"laughing"

NOT binds more tightly than AND/adjacency, then OR. Parentheses preserve nesting.
Quoted values use JSON escapes. Bare words and `text:` are literal FTS phrases.
`rating`/`grade` use native 0–3 thresholds, not Lightroom's 0–5 star scale.
Dates accept calendar years, months or days, including inclusive ranges. The
compiler turns the upper endpoint into an exclusive next-period boundary.
`focus` uses the indexed focus score. `person` currently uses named keywords and
the keyword hierarchy because the index does not yet persist named face identities.

All boolean filters compile into typed `index::Predicate` values with bound SQL
parameters and identical filtering for search and facets. Missing leaf metadata
is false, so NOT also matches absent values. Scope is applied before pagination.

A single positive conjunctive semantic term becomes `Query::semantic`. Execute
it with `Index::search_with_semantic` and a `library::SemanticSearch` provider
(re-exported from index, implementable by the embedding owner). Semantic terms
under OR/NOT or multiple semantic terms are rejected, not approximated. Literal
FTS predicates remain conjunctive when combined with semantic retrieval.

The original Lightroom AST serde shape is unchanged. Unsupported imported rules
are preserved but fail compilation explicitly. Display uses a lossless `@json:`
escape for AST shapes without a native textual equivalent. Parsing reports byte
positions and bounds input size, node count and nesting depth.

## Verification

    cargo test -p library -p index -p cull -p import-lrcat --release
    cargo clippy -p library -p index -p cull -p import-lrcat --all-targets -- -D warnings
    cargo fmt --check

Tests cover parser round trips/errors, synthetic SQLite filters/facets, nested
project scoping, semantic-provider filtering, safe album operations, document
round trips, importer identity translation and cull sidecar reconciliation.
