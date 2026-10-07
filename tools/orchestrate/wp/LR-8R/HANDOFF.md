# LR-8R port handoff (in progress)

Base: `392c2156aaed72237b594fc640b4b3238d6ad2c1` (gate/b51 candidate).
Scope: port only; recovered Machine A LR-8 and LR-13 rulings read in full.
88 source commits: 51 LR-8/8b/8c/LR-10, 12 approved decoder hotfixes,
then 25 INT-1/LR-12/LR-13 commits. Source messages and existing trailers preserved;
no new Co-Authored-By trailers added. Exclusions follow task exactly.

## Conflicts and resolutions

e2bff2a5: pipeline-cpu/render.rs: retained LR-11b split_local_point_colors stage; apply imported local hook separately to point_groups and remaining_groups. Reviewed rerere result.
07337d42: tests/lrcat.rs append conflict: retained added split-preview regression (its original RED commit was outside requested range).
a6b8ab39: fixture README append conflict: retain both approved LR-8f/g provenance and LR-13 JXL wrapper provenance.
e4fadad9: export.rs: retain main mutable segmenter and required AI availability checks in export/print; do not bypass segmenter load for external proxies. Imported proxy mask-hook planning remains deferred LR-13b.
5d5be036: fixture README reorder conflict: use final source provenance exactly, retaining LR-8f/g and corrected JXL history.


## Port map

| Original | Port |
|---|---|
| `1d22a69ed86fed4a64f793efc0e6da297c8dd2ec` | `666b9d486b7b6bb6963f8af562b1aa52a04a86e6` |
| `153012a7b51ac84183af3d2c1a08489372f35c11` | `a1c8343041bb074754c766cd6e9bdf05ee9ef567` |
| `1e87aa89cf26dc7824e8ea0973eb5ec71c3a1984` | `3433246b16b0a2186d1ce9192d120349fe118a5e` |
| `2effef6b62c33860bcfa12efe4be2fbf0fa8e934` | `e374d096037796fde5bb548f141c8ec611dd6b22` |
| `4cb0ee7411cf2f1595e0b214727bb4c39569d8e0` | `1dcbe4fe079032b55b5a13fff622563b6be526e1` |
| `3c3553150f0bc47a4ecadfecb54ab8b558aaae99` | `4795c7fd5f51c7429c5a14d33271210b65e9f166` |
| `1620b92f0662f2a89a82d91ff2be41f5255ca184` | `ec5ee6d79bd138aab57f1640be94f571e194520c` |
| `9fde28afef83962320975128e96e3b43afd4baba` | `6399c21691d38aa415afedce6fb5b42b8d4043b9` |
| `928e84d6b75860ee4c17cbceb878a4643e876dcd` | `edf066f992152d4ecda58217b6cd20b39ba0b22b` |
| `dfb44e6ead32f3ec62e871dca460133e1273978d` | `0d698e23c54b94f90a3446661a2daf89162f9aba` |
| `1d0c159ca24ee19060a6b8b86f9a35b9ab3252ea` | `5dd4d6e18e185f402e7e3fb690035554cc0a5a09` |
| `16b8566439c5be1a794e69a96ce643e01a65cb0e` | `695a801ea4580720825deee80ada4b9a8ae1e0ff` |
| `80b5bc192b339a344e538eb7be81106149eebde5` | `164da9d7b08824d398ee01ee052f57917908ff75` |
| `2359d08e18236c522869c80d5bb70e1457f96542` | `f6a3f94c740711574e77f4bee9b484464b642766` |
| `eda70d0ec36ef357641c87c1085e73e4c4f7266f` | `0c12d9cdfd1317d3db44e0d5367111add335c89d` |
| `94de3f31e436410cbbde8f8c5d3f5230b1b0ac79` | `ccda096c33ca37a839ea4a939c7c8d042248f210` |
| `5c2aa64f965e77921ecbed061a2e93ad3c63f215` | `d54ccf217778f6d5e8f563cdeda02bb5c74d0179` |
| `826dd31eb51d0e64872fe6ed7bac5de916a0de42` | `9c69d92dd98ec665b36cf16ad63696f41004f9ec` |
| `e2bff2a5b9a15d5da22bdbdd45d8ff8f0ef6b616` | `3f3e4c8af771c6a1767443824ebd9f2107a75180` |
| `78869eef56c2117c08a04b13595c46ae12f03a64` | `f0ab1459dcee37191c06adf122d04ddb033e74b1` |
| `56dae5ef6adfc2ac432dac9dfcd7e3ff4e4e8af5` | `027f4e3b8cdda87682356cde04be0f236bd03608` |
| `f0326001ebbbf546ce12942d5198867e896caf00` | `0d547d66f013488660547cb11671f2e48858f321` |
| `dcaeb44bc5f0f8c4379d5d518a91d919497b6eee` | `4a5a323e86ee5c1b7d88ad6e4df5cf9ef55eb9ab` |
| `0f4feff411bdc295a94e46518aaff2b47a73ed67` | `6b0fa4287f73b37852e3ee9ff7791d427fd5850b` |
| `b2986aed647e61e0a3e6f0dc600c9c4008dfb0e8` | `4af8fd4488e19e7eca11bfd9b6e267e51d5b15e5` |
| `67cdfa618ad100f3a682a9be3dd9c8db11c2e7c9` | `985ea86ae8dbe10847dee407f85e4da6c48358cf` |
| `b0a4aa842ef1e66e0b19e9ba50abf57dab553b6a` | `485194884cafb33351f85bd999d8fe40848070bf` |
| `29f12feed346f04fe5c883c8d34425e17fce933c` | `27664f0b6f4346b11e7838c3cd3a464df825c93e` |
| `010156fdfbfcd29ad234d78c49934d188c6f7bdd` | `73dbfeeb7b29f2375d002bbea41edf62b6c729ae` |
| `5a05974b3845cae62c15d942371bb9fcdf44d930` | `a0546dbf5b3eddc3d39e38fc120f25b973520a6c` |
| `6808dc42aff24724f838c1cddb05df84fea02483` | `c4c60e92ff54b32b82d110e634037100347b950d` |
| `0e8fd03787a8b9e1dcacf789cd829278ba30ec68` | `fba5eaf0c94f15c5cddbb8648bace4669a77754b` |
| `4d899b553103422673c4ca7c5ce52f9d33513891` | `1294093a4aa9b8cbb0ff122b3cf039d0bda00486` |
| `d586055ba52793d3f550f09800b2357c2444a071` | `8b6d33df3543a38fb7f91308b941b726318590f1` |
| `5f7ff0ab019c7846ba5f81909ce85ed2da70c7ab` | `75d048ab0d411e48250f1be17990c54ce0fb4b5d` |
| `6c2dace3686160c1b7b29057dd5af69f915c6ef3` | `76be668d8bc2fdbebdfa8289612b01fc4dabf493` |
| `660ff7dbcea987744376824735a542ee6aee78d1` | `1074cdfc14660e7e0928597d27e201632543b050` |
| `6e6c7286a836da620cc1e372542aac0b4ea441af` | `f38f86f07d5bb429b8f83d466a9c9ff81223fe24` |
| `548e24346361c7339b7fe8e50e54398f08621c96` | `8cfea55733e4be0c61ab8fbeffa255c033b0469b` |
| `218642e909f46e0a96b62c01eddfb7dbceec4d00` | `40b5a9816e08319186626c61d7f033eb423b78bb` |
| `1e75421c97033b41409c5db5b2721b9f2a4bcbca` | `3686e18c30b595022b8fc830cf6ae89fd792fc39` |
| `57f5da01daa581109ccd9d3393ddb68fafa7aafd` | `3f1705c9bdcf63ab7c9758e39a808b83e1d423c2` |
| `bc3b454cdcdacf256a2eb5d0d587e18d7df5c349` | `a2d6de933fffc295ca1edb0a67ed5f9d7ed1af7a` |
| `957167eeb8a33380f2c1cb2ee522dcbfd1cf7d47` | `e9bd8d29058bffb99722fb50c6d81d39aece5b29` |
| `a47096eb87898a44b437dfcc1384eff2b726caf6` | `a8d456fc930a9392ed7003a306ac314e22ea9adc` |
| `581e494ae1d9dbe3e3647113a2ce6308ca7081a5` | `35047da96fc8fc56e100a17e09d8e7e4f1c412d7` |
| `3e86ef5ea323c7ad4c407501ca79bda797fc39a5` | `d317017406c20b61bc7ec21a749723f034872479` |
| `1934bb1f31677eace20fd6b5622cb43de100610e` | `d9b9ce7e1bdb9f6d8ccc90f2c0f1deb36add2673` |
| `8e4d8e6dbf17d592b1682c9983975209a6138636` | `97601a577213f8a17ec0cfe38ab25f1043ffe464` |
| `0c1a52c3d02c7a2a5f181d95f9496dd4f10464d6` | `46265e9d6de055330c184e211581c59c1f11fb4b` |
| `afe97d4c7cae8310b07f141fb137cdaa51187abe` | `14204a9bd9e84f2fb9a719bea765f807c9704a72` |
| `6417ec4f010f62ead7811679acdfd17eb76f3e8a` | `bc94887ecbd52b593b59d6ad4ba5955bd26b11e2` |
| `3256ed56cff0ad4ba6fe8ff4b142113a5155763d` | `ee3a108fc93bd3e6ba5919da0ee82fdf380cd26b` |
| `85958b355df60e2c5991b264f1c719ae5a4bb5cb` | `b79af9c4e3f7e5b91ce96330dfd698fbd5ae467e` |
| `ec2f312ac4bfc8717f7a103c23b000541cca025d` | `af031e7d3484321e7f18b2d7d5493dff4c05676a` |
| `46965d1cfca6a95b1901e102586ea3685aaa2bdd` | `5fed2c780478ae2681a44341d5c9d196fb55edbb` |
| `3a60b599d987d072ac8f48e2564b844149750edd` | `0a4a47a8df9ab9b50f0fb12cebd3b603be987c46` |
| `6a5de47e8391a6bc9f843ffa1c02e0de4faa8adc` | `9bc03578ee66f944dd5a32441f6ba6b4f4cc020f` |
| `bb1bc53f9fe420e5af6bff1796a341b8a6c6c8dc` | `7882fe1615e184b0dc6cff9f86e0750e57b41451` |
| `2ef6ab56f71a8c9ff634790acd77d588f1e72685` | `59bf1d54b1340f6b0ad8293640d05d55aea0a4f4` |
| `b8f128f8af011aef1f8860d2da28425b4644feca` | `e79d4607a1af0a3bcccecfa6accfbe1d4526fcb6` |
| `54df428a54631994051efd4313f74672a09a2bdf` | `b150489ebc4b684fcbc99b61eef862d35ad10c5d` |
| `38817f5690f3c79ef7f5d7135250db95d72dd207` | `57ef9ae214ca51fb418be6535303e9cd34237a72` |
| `5ce96b45ab862705b74eb57e5dcc2ef18f1242a0` | `7dea75cfa4afbe6acda181d6f9be1d2a43a24431` |
| `b47a482b170102c7431b0e98a8d9690a3dd34dd9` | `ba6eba082710e381f1b849f13e7e081e41eeac68` |
| `07337d4263f6691ec08f434c53c675843ea8790c` | `9e8b31ef8503f29f671d20d27b66dc4c8f8e0ba6` |
| `bd4c71c7e0190aabe55f427aed161ac80ca02a48` | `f9022b9644503a87a36bf5f8cfac08ae71c433f0` |
| `a6b8ab39b6d8a183531340f955bcba6bd192a1f2` | `299eaa18ce5afde700e45c00921223cb028ee383` |
| `a1c269df3092a8e7f46eac4ae91de9961bcf25e5` | `4f4ac5774b66d68023a8a629b6d16b0bd6fd4541` |
| `d986db0dbf36e60f769618650051b0518e03713c` | `1e0faff34a0084e2789f521be8685d1cf90ff855` |
| `7907804b61081f7a6b407f812fb1585e97f2e3cf` | `b712fd3c00b776d678a3f907c67210173c248830` |
| `9fb8b5320045c4eec1def8ca61768f80990f0a0c` | `8494a226cb580b5f5c17d581ac9c35713d746732` |
| `e4fadad905679839277b96d7ffd8ccff80f44234` | `4dd6a6fd8a9655c6c5c9697dcd4c91364eed47b0` |
| `2aaf12837d17738eccc90b73599d352af55fbc15` | `35df7477df2a9b811533a3cfe80b74a2d565c8ed` |
| `df72fcb77019b1638788d1e1f62c6fb5a81086c4` | `beb48639b1df4d9a01f6ca234e2e9a3c7c44c606` |
| `ab92459021a12d5d39a6babc40703032f3a0599b` | `ebfd9023938c3624cd617bba7284f866a904de4d` |
| `8c2b8e10c927d20b923942dedd9605700934f93f` | `a2d6994ce223f40835bcf66e365720f4c328dbe9` |
| `87e70eec001b8d1668d0d3ab7f0785b5d6a62dfc` | `b97c458d9256d64f0dce6e19cbe0b4b5a776c1ee` |
| `cca99b5bcbeeaeaa15e7e4f17407bd3dde67d8da` | `36f4250a959287d44b513ced27f014761495dac7` |
| `c5132de657d7b9989d388f941571e679852bb7ba` | `f8cf128aa930302a037c75dc7323822eb1d1d651` |
| `95cb645ce281bf7f5c2728ef85785db89a3c61e6` | `0f5ae2b01841076eea21661063142c13331b8457` |
| `70921d37f044468c478152535f609fd8c64cfc7f` | `15ab199d58cbd9e26418ae59d05f0f432941e95d` |
| `e2438c726ba2de63657e30ca2903fb1d88edbb0c` | `97da44da3bafa8c6a6c88062a6d55a320b62309d` |
| `7df6f558f49752aa00c8828d911ecba2c60a1742` | `7f083f03eafb4a099f52e08d4f51e04a597b0d95` |
| `348b74f7d4ea7d5f58fd0bfd1a0aa6c8635fb6c6` | `7a6ecc63f311adc2762bc1aeee166319289fb5f0` |
| `5d5be036a055ee3375855287ffaa39e28b64b3e2` | `71bcac78683463901f3c89cfbb0a8bfb4f8553e6` |
| `e07fe9bc9ba04e227a1fd5afed35d42727341e6c` | `9551060fef9497a8f332d9065758bbb60288de02` |
| `124c03bcf6fcb14a318fc555574f96e62f245595` | `726c334a4ca9121dcc75e8dcfbcfecd304839e29` |

## Review items deliberately left open

- B3: scope of embedded-profile fallback, Native Adobe-name dispatch, parse fallback, EXIF illuminants, substitution notice; M11 fixture/0.2 restoration belongs with B3.
- B4 / LR-8d: Native vs Adobe BaselineExposure split, including removal of inherited golden-test calibration override.
- M2: unclipped HueSatMap highlights before exposure.
- M3: scalar thumbnail route still develops full proxy before reducing (LR-13 partial improvement retained).
- M4: relinked originals normal GPU/lens route.
- M5: cache library listing.
- M6: original catalog filename for display/export.
- M7: image-id proxy copy ownership.
- M8: common orientation/geometry convention and lens resolution/order.
- M10: normalized local edits across proxy-to-original relink.
- M12: Adobe DNG SDK ACR3 licence attribution.
- Minors: copy-toggle accessibility identifier, production environment override, library destination when SSD absent.
- LR-8h: stream one compressed tile at a time and allocation evidence; approved decoder is preserved byte-for-byte.
- LR-13b: export/print imported mask hooks; real depth/retouch support; raster errors in thumbnails/export; defer profile parse to Adobe; persistent notice without per-frame lock; mask availability/plan-version cache key; same-level thumbnail comparison; admission assertion justification for ab924590.

## Golden boundary

No stored import/native pixel golden files or existing import fingerprints changed by port.
Inherited source `e2bff2a5` changes `pipeline-cpu/tests/golden.rs` to zero
BaselineExposure during native golden rendering. This is Machine A B4, explicitly
left for LR-8d; it is not evidence that ordinary Native rendering is unchanged.
`6c2dace3` adds ordinary-import neighboring-preview byte-equality coverage.
No existing assertion or bound has been changed during the port. Gate results pending.

## Verification

Full command logs kept outside repository in `/tmp/LR-8R-port`.

| Attempt | Command | Result |
|---|---|---|
| 1 | `cargo test --release -p tessera-ffi --lib lrcat_combined` | Build passed (7m11s); filter matched 0 tests, 296 filtered; not counted as test verification |
| 2 RED | `cargo test --release -p tessera-ffi --lib` | 284 passed, 1 failed, 11 ignored; new `lr8r_oriented_offline_proxy_accepts_matching_imported_ai_raster` rejected valid raster; INT-1 passed |
| 3 GREEN | `cargo test --release -p tessera-ffi --lib` | 285 passed, 0 failed, 11 ignored (8.48s) |
| Precheck | `cargo fmt --all -- --check` | Passed |
| Clean | `cargo clean` with `-p` for all touched crates | Completed before final gates |

Environment: `PATH=$HOME/.cargo/bin:$PATH`, `CARGO_BUILD_JOBS=5`,
`RAYON_NUM_THREADS=5`, `CARGO_TARGET_DIR=$HOME/.cache/tessera-target/LR-8R`.

## Port integration regression and fix

| Finding | Code | Test |
|---|---|---|
| LR-5b's original-only mask extent helper rejects injected oriented LinearRaw proxy rasters | `c77cb368`: `lrcat.rs` uses approved metadata reader, active crop and catalog orientation only for offline proxy injection; ordinary originals retain main helper | RED `814517fb`: synthetic 12x10 proxy, orientation 6, accepts/pins a 10x12 injected AI raster; full FFI library RED → GREEN |
| LR-11b Point Color ordering must survive imported-mask hook addition | `3f3e4c8a`: invoke hook separately for Point Color and remaining locals | Existing LR-11b stage tests retained; INT-1 nested local edits/orientation passes |
| Main AI dependency availability checks must survive LR-13 routing | `4dd6a6fd`: keep mutable segmenter and export/print load checks | Existing LR-5b checks retained; full gates pending |

## Added port commits

- `814517fb`: RED oriented proxy raster integration test.
- `c77cb368`: oriented active proxy mask extent fix.


## Independent port review

Read-only reviewer checked approved decoder equality, LR-11b local staging,
LR-5b group availability/export checks, LR-9c groups, lockfile boundaries,
numerics/accessibility/profiler changes. One P2 found: inherited LR-13
`MaskShared::unavailable` incorrectly requires full-resolution dimensions after
successful lower-resolution LR-5b regeneration. RED `3aeb1d33` added a notice assertion to the existing LR-5b lower-extent regression; it failed 0/1. GREEN `bafe4684` aligns notice readiness with validated `Ready` entries; focused rerun passed 1/1. Existing raster/pixel assertions and stored-extent validation remain unchanged.
Inherited identity-catalog-orientation non-proxy resident unwrap concern is
also present at source `124c03bc`, so retained for routing follow-up with M4.

## Feature port checklist (final gate evidence pending)

| Finding / required feature | Code | Test coverage |
|---|---|---|
| Approved lossy JPEG / JPEG XL LinearRaw admission and bounds | `raw-decode/src/lossy_dng.rs`, exactly `38817f56` | `raw-decode` lossy_dng, lr8e_safety, lr8f_safety (includes LR-8g/h) |
| Offline proxy discovery/import/copy/relink | `import-lrcat/src/smart_previews.rs`, FFI lrcat/catalog, sidecar store | FFI `offline_proxy_import_develop_copy_and_relink_preserve_lightroom`; read-only store; UUID lookup |
| Catalog orientation consumed once before normalized edits | image-core source, pipeline-cpu render | catalog_orientation, lrcat_linear, FFI lrcat_orientation_tests; INT-1 |
| Current LR-5b injected raster extent validation | FFI lrcat/lrcat_masks/masks | LR-5b RGB/RAW extent tests and new LR-8R oriented proxy regression |
| Current LR-11b B2/B3/S7/S8/S9 semantics | Base sidecar/import/render rules retained; new CPU hook keeps split staging | Existing lr11b tests plus INT-1 nested local operators on oriented Adobe proxy |
| Current LR-9c cloud and regenerated-mask report groups | Base FFI lrcat report code retained | `lr5b_import_report_shows_cloud_group_and_regeneration_notes_together` |
| LR-10 Adobe proxy rendering | pipeline-adobe, image-core embedded profile dispatch | embedded_adobe, embedded, dcp_render, lrcat_linear |
| LR-13 thumbnail/loupe/analysis/cull/Develop routing | FFI imported_proxy/preview/analysis/cull and image-core smart_preview_render | `lr13_imported_jxl_proxy_reaches_app_preview_analysis_and_develop`; thumbnail-size test |
| Proxy export with quality warning | export lib, FFI export | export lrcat_jxl; FFI offline proxy export and LR-13 app route |
| Develop notice wording and HDR conditional | FFI imported proxy notice, Swift Develop session | LR-13 notice wording/no-op HDR and FFI minimum-notice tests |
| UI source counts, badges and copy/import controls | Swift import sheet/controller, ThumbnailCell, EngineLibrary | Swift LightroomImport tests and full Swift gate |
| Main numerics, accessibility identifiers, profiler hardening | Base ENG-1..4/B5-50/B5-51 retained in merges | Workspace tests, Swift tests, strict build |

### Inherited numerical fixture/expectation differences versus base

| File / expectation | Difference already in source | Source commit |
|---|---|---|
| `pipeline-cpu/tests/golden.rs` native fixture calibration | Set `metadata.baseline_exposure = 0.` before rendering; golden image bytes unchanged; B4 remains open | `e2bff2a5` |
| `pipeline-adobe/tests/dcp_render.rs` profile fixture | Missing tone curve becomes explicit identity curve | `1e75421c` |
| Same profile fixture | Add explicit black-render policy tag 51110=1 | `bc3b454c` |
| Same profile fixture and `tiff_profile_changes_final_cfa_render` expected input | Scale red matrix diagonal only; expected `[0.2; 3]` becomes `[0.21781155; 3]`, tolerance remains 0.0001; M11 remains open | `3e86ef5e` |

No new numerical expectation changes or golden re-pins were introduced by LR-8R.
`01a0cc8a` adds neutral metadata fields to the base LR-11b synthetic fixture
(`baseline_exposure: 0.`, `catalog_orientation: None`); every assertion is unchanged.
Workspace attempt 1 stopped at compile E0063 in this fixture (no test execution).

Additional port commits:
- `01a0cc8a`: neutral RawMetadata fields in LR-11b fixture (compile adaptation).
- `3aeb1d33`: RED regression for regenerated-mask availability notice.
- `bafe4684`: GREEN readiness notice integration with current LR-5b.

The ordinary `cargo clean -p ...` reported 0 files because the lane held release
artifacts. An explicit `cargo clean --release -p ...` was therefore run before
workspace attempt 2; both logs are retained. This avoids claiming a clean build
from a no-op package clean.

## Final gate attempts (continuing)

| Gate attempt | Result |
|---|---|
| Workspace 1 | Compile failure E0063 at `tessera-ffi/tests/lr11b_local.rs:329`, fixed by `01a0cc8a`; no tests ran |
| Explicit release clean | 913 files / 3.2 GiB removed across all 13 touched crates |
| Workspace 2: `cargo test --release --workspace --no-fail-fast` | Exit 101: 3,357 passed, 1 failed, 106 ignored; 654 summaries including doc-tests. Only failure: `previews::tests::raw_without_jpeg_is_rendered`, 4.30435325s against unchanged `< 3.0s`; pixel assertions passed |
| Final formatting: `cargo fmt --all -- --check` | Exit 0 |

Workspace attempt 2 passed INT-1, both LR-8R regressions, existing LR-11b tests,
FFI Lightroom integration (18 passed / 1 ignored), import goldens and native RAW
goldens (subject to inherited B4 override). Streaming import scale passed in
102.94s; no rerun was needed for it. Complete previews crate serialized rerun
started as required; no CI override or bound/exclusion change was used.

| Serialized previews attempt 1: `cargo test --release -p previews -- --test-threads=1 --nocapture` | Exit 101; lib 22 passed, 1 failed, 3 ignored; same timing assertion, 3.596126042s vs unchanged 3.0s, pixel statistics unchanged. Cargo stopped before the integration suites. Command including standalone feature rebuild took 86.00s. Machine load immediately afterward: 79.26 / 69.74 / 75.92. Gate remains unresolved. |
