# Culling, Selection & Collections

Research basis: [research/workflow-research.md](research/workflow-research.md) (forums, educators, vendor docs; no formal survey data exists, so the conclusions are qualitative).

## 1. What is wrong with flags + stars + colour labels

- Three overlapping systems with no shared meaning. Ratings mean different things per person; labels are used mostly for *edit status*, not quality.
- Inconsistent persistence across apps: Bridge and Lightroom store rejects differently, flags were catalog-only for years, labels are free-text strings that must match exactly between apps.
- The actual dominant workflow is simple: one pass of reject/pick, then a coarse grade on the keepers. Most people never use 5 levels of stars.
- The slow part is not the metadata, it is the *looking*: slow 1:1 rendering, hand-comparing bursts, checking every face for focus and closed eyes, and decision fatigue.
- Modern AI cullers (Aftershoot, Narrative Select, Imagen, FilterPixel, Lightroom Assisted Culling, Capture One Assisted Review) all converge on the same model: separate **technical defects** (objective, machine-detectable) from **taste** (the photographer's call); group near-duplicates first; show per-face focus/eyes directly on the image; only map to stars/colours on export.

## 2. Our selection model

One decision, an optional grade, derived status, one free mark, and read-only AI signals.

| Field | Values | Keys | Notes |
|---|---|---|---|
| **Decision** | Reject · Undecided · Keep | X · U · P (auto-advance on) | The only thing the culling pass asks for |
| **Grade** | none · 1 · 2 · 3 | 1/2/3 on a Keep | Optional; "keep", "good", "best". Three levels cover client selects / portfolio / hero |
| **Status** (derived, read-only) | unedited · edited · exported · published · in album X | — | Computed from history/export log/album membership. Removes the main reason people used colour labels |
| **Mark** | one user-defined coloured mark per image (user names the set, e.g. "needs retouch", "client favourite") | 6–9 | Replaces colour labels for personal semantics; the set is per library and exportable as label text presets |
| **AI signals** (read-only) | focus score (global + per face), eyes open/closed per face, motion blur, exposure/clipping, noise, duplicate/burst group id, "best of group" suggestion, aesthetic score | — | Shown as overlays and sortable columns; never change the Decision without an explicit user action |

Stacks and virtual copies are unchanged.

### 2.1 Interoperability mapping (XMP)
| Ours | Written to XMP | Import reverse mapping |
|---|---|---|
| Reject | `xmp:Rating = -1` + Lightroom reject flag | rating −1 or reject flag → Reject |
| Keep (no grade) | `xmp:Rating = 1` + pick flag | pick flag or 1★ → Keep |
| Grade 1 / 2 / 3 | 2★ / 3★ / 5★ | 2★→1, 3–4★→2, 5★→3 |
| Undecided | no rating, no flag | — |
| Mark | `xmp:Label` text using a per-app preset (Lightroom/Bridge/C1 names) | label text → mark by name |
| AI signals | private namespace `cull:` + optional `cull|…` hierarchical keywords (off by default) | ignored on import unless from us |

Import from a Lightroom catalog also maps existing 1–5 stars, flags and labels through the same table and offers a preview of the result.

## 3. Culling UX

- **Instant previews**: the culling grid and loupe use the embedded camera JPEG until our render exists; zoom to 1:1 never shows "Loading" ([08-performance.md](08-performance.md)).
- **Group-by-group review**: bursts and near-duplicates are auto-grouped; ←/→ moves between groups, ↑/↓ within a group; the suggested best frame is pre-highlighted; one key keeps the best and rejects the rest of the group.
- **Face strip**: every detected face in the frame shown as close-ups with green/yellow/red focus and eyes-open badges (Narrative-style); per-person filters ("show frames where the bride's eyes are closed").
- **Defect sweep**: one-click "reject all frames with missed focus / closed eyes / blown highlights" using adjustable thresholds, always undoable and shown as a reviewable list before applying.
- **Compare**: 2–6 up with synced zoom/pan, swap, and a "choose this" key; survey mode that eliminates by dismissing.
- **Sort and filter** by any AI signal, decision, grade, status, mark, group.
- **Learning**: the culler learns the user's decisions over time (features: AI signals + embedding) and reorders the review queue so likely rejects are handled in bulk and likely keepers first; the user can switch between "assisted" (suggestions only) and "automated" (pre-filled decisions to confirm).
- **Tethered/ingest culling**: scores are computed as frames arrive so live shoots can be culled during the session.
- **Keyboard-only flow** for the entire pass; decisions write immediately to sidecars with a single global undo stack.

## 4. Collections: who uses them and what to build

### 4.1 Findings
- **Heavy users**: landscape, travel, wildlife, stock and hobbyists with multi-year archives. Uses: portfolios, print and book sets, competition entries, smart collections by keyword/rating/lens/date, publishing to SmugMug/Flickr, syncing to phone/iPad, year-in-review.
- **Light or non-users**: many wedding/event photographers work per-job from folders only; sports/news select in Photo Mechanic and rarely touch collections.
- **Complaints**: folders vs collections confusion; Delete inside a collection silently removes only from the collection (or, worse, users expect that and lose files); virtual copies orphaned; smart collections and sets do not sync to mobile properly; nested smart rules hidden behind Alt-click; publish services bound to one catalog.
- **Praised designs**: Capture One Projects (a group whose smart albums search only inside it), Apple/Google "smart albums as saved searches", Photo Mechanic's fast folder-first flow.

### 4.2 Decision: build a small, folder-friendly version
| Concept | Behaviour |
|---|---|
| **Album** | Manual list of images with its own sort order; an image can be in many albums; membership is stored in `library.json`, not in sidecars |
| **Smart album** | Any filter/search saved with one click; rules are the same grammar as the filter bar, with visible nesting (and/or/not groups) |
| **Album group** | Nestable container; a smart album inside a group can be scoped to the group's contents (Capture One Projects behaviour) |
| **Basket** | One key (B) adds to the current target album; the target is shown persistently |
| **Safe delete** | Delete inside an album removes from the album only and says so; "Delete from disk" is a separate, confirmed action everywhere |
| **Publishing (later)** | Link an album to an export destination and track changed/unpublished images; publish state travels with the library, not a machine |
| **Status integration** | "In album X" is a derived status, so the filter bar can find images not yet in any album |
| **Sync** | Left out of v1; when added, albums and smart albums both sync as first-class objects |

Not built: Quick Collection as a separate concept (the Basket covers it), collection-level develop sync (use presets/batch), Lightroom's distinction between collection sets and collections (an album group is just a group).
