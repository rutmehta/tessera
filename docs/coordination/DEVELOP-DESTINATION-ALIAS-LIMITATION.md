# Destination-key alias: bounded design and scope recommendation

## Finding

Current `recipe_write::destination_key` canonicalizes the existing `.edits` parent (or canonical image parent plus `.edits` when that directory is absent), then appends `Sidecar::paths(image).recipe.file_name()` as a lexical `PathBuf` component. This is correct for the pre-existing exact sidecar-path contract, including same exact stem across extensions, but it is not a complete physical-file identity on case-insensitive volumes. On the observed APFS volume, `photo.json` and `PHOTO.json` can resolve to the same inode while their Rust `PathBuf`s remain unequal. Since the sidecar may not exist when the first lease is reserved, comparing sidecar inode only when present is insufficient.

The bounded invariant should be stated accurately as: one active lease per current canonical-parent + exact recipe-filename gate key. Do not describe it as universal physical-destination exclusivity across every filesystem alias.

## Why a one-line normalizer is not a correct fix

- Apple exposes volume case sensitivity (`_PC_CASE_SENSITIVE` / `VOL_CAP_FMT_CASE_SENSITIVE`), but that only answers whether case distinctions matter; it does not return a canonical filename key. `pathconf` applies to an existing path/volume and is useful for choosing a strategy, not for mapping arbitrary absent names.
- Apple documents APFS as normalization-insensitive, preserving spelling but looking up normalization variants, and its case behavior as Unicode 9.0. HFS+ uses older Unicode 3.2 behavior. A Rust `to_lowercase()` or a current generic Unicode fold can therefore over- or under-merge names relative to the target volume. Foundation/CoreFoundation folding APIs implement Unicode string folding and normalization, but the reviewed docs do not promise that their current tables exactly match every mounted volume's filename collation version.
- `stat`/`canonicalize` of the recipe sidecar can identify aliases only once that recipe exists. It cannot prevent two different spellings from separately reserving a not-yet-created sidecar.
- Probing equality by creating a temporary file in `.edits` would mutate the user collection, create watcher-visible artifacts and leave crash-cleanup concerns. A per-directory lock held for the full edit would avoid the spelling problem only by serializing unrelated images, which is explicitly outside this bounded design.

Relevant primary references:
- Apple APFS filename behavior (case sensitivity variants, normalization-insensitive lookup, Unicode 9.0): https://developer.apple.com/library/archive/documentation/FileManagement/Conceptual/APFS_Guide/FAQ/FAQ.html
- Apple filesystem guidance noting default APFS is case-insensitive but configurable: https://developer.apple.com/documentation/technologyoverviews/files-and-directories
- Darwin SDK `sys/unistd.h` defines `_PC_CASE_SENSITIVE`; `sys/attr.h` defines `VOL_CAP_FMT_CASE_SENSITIVE` (local SDK headers inspected).
- Core Foundation documents `CFStringFold` as Unicode case folding and notes normalization is separate: https://developer.apple.com/documentation/corefoundation/cfstringfold%28_%3A_%3A_%3A%29

## Recommendation for this Stage C slice

Do not add guessed lowercasing or create probe sidecars to the current lease branch. Keep the existing exact-path key behavior, name the limitation in the Stage C contract, and treat filesystem-equivalent filename aliasing as an independent prerequisite before making a stronger “one editor per physical recipe file on all supported macOS volumes” claim. The bounded Stage C acceptance still covers ordinary exact-key same-stem collisions (`photo.jpg` + `photo.png`), owner authority, and participating Engine writers. The demonstrated `photo.jpg` + `PHOTO.png` alias is a real limitation on default APFS and should remain a tracked follow-up, not be silently called fixed.

A correct follow-up needs a deliberately chosen policy/API. Candidate investigations, not implementation recommendations:
1. Define a supported-volume/name-semantics contract and use a volume-specific canonicalization algorithm with pinned equivalence behavior; validate it against case and normalization variants on APFS case-sensitive, APFS case-insensitive, and HFS+ where supported. Treat unknown volume semantics as fail-closed rather than guessing.
2. Migrate recipe sidecar naming to a stable per-image identity, with explicit legacy-sidecar discovery/migration and collision behavior. This avoids deriving namespace identity from a filesystem-collated stem, but is materially broader and changes the established path contract.
3. Use an OS-native per-destination reservation primitive only if it can establish identity for absent target names without writing user-visible sidecars and without holding a directory-wide lease. No such no-side-effect public API was identified in this bounded review.

No code, build, test, or filesystem probe was run for this design note. The only runtime fact cited above (the two spellings resolving to one inode) was provided by root's prior isolated-volume verification.

## Coordinator disposition and observed evidence

Machine A retains Stage C's existing-key contract and tracks physical filename aliases separately. This limitation predates the lease implementation and is not counted as fixed. No guessed normalization, migration, or user-sidecar probe is part of the candidate.

Root used a disposable system-temporary directory, created `.edits/photo.json` containing `probe`, and checked `.edits/PHOTO.json` without creating the second spelling. Observed `upper_exists: true` and `same_inode: true`; `Path.resolve()` returned distinct suffixes `photo.json` and `PHOTO.json`. This qualifies the filesystem alias, not a native two-editor acceptance run. The fixture was disposed normally; no user collection was touched. Existing OwnerBaseline defense remains required, but does not turn the lease into all-writer/filesystem-CAS protection.

Next action: choose and review a supported filename/volume identity policy or an explicit sidecar-identity migration contract, then establish a case/normalization test matrix. Do not change the current key during a frozen acceptance run. A owns this follow-up; B's reserved Document task is unaffected.
