# Independent opaque-owner RED/API review

Approved the bounded RED/API checkpoint at c099335882b0e677d75d6069a1079e789e7efaad. Read-only review; no tests, compiler, native decoder, app or GPU execution performed by reviewer.

Independently reconstructed SHA256 for all 9,231 Git blobs for BOTH 243eacfee83a3a2bcce99ff65285e06558fee28f and c0993358 and matched their compile source manifests. All completed before/after source/input/artifact pairs match. Runner and exact-name oracle hashes match frozen inputs. Rehashed all 257 Cargo-emitted dependency artifacts and exact test binary; no mismatch. Binary SHA256 is 3a3e746d819d0304470af9a5f8090fbc2e0e433b3b6bc78b166218bc4d704a82 in both attempts. Cargo artifact JSON identifies production rlibs, avoiding stale dependency glob selection.

Compile direct exit 0 and exact five protocol tests direct 101 verified in both attempts: zero pass, five fail, zero ignored. Success fails on explicit Unsupported; the other tests expect typed cancellation, Decode or Io while unchanged stubs return Unsupported. The classifier's initial Unsupported row can pass before its malformed Decode row fails. Later table rows, pointer/capacity, metadata/projection and cleanup checks are latent, not executed proof. These are meaningful missing-behavior boundaries, not compile errors or evidence that cleanup is implemented.

243eacfe downstream positive succeeds. Clone correctly produces E0277 at clone.rs:3 while the old oracle expected E0599; runner stops, preserving the oracle failure. Seven remaining negatives are unrun in that attempt. The correction changes ONLY expectations.json's Clone code/text; all product and test bodies are identical. The two external runners differ only in pinned HEAD.

c0993358 API wrapper completes 0: positive downstream compile 0, eight negatives direct 1. Inspected structured primary diagnostics and exact fixture spans: Clone/conversion/Default E0277, immutable metadata/opcode mutations E0596, unavailable parts/samples E0599, private fields E0451. Each is the intended concrete closed-interface failure; none is a missing crate/import or generic setup failure. Compiler commands use the exact compiled production rlibs. Individual source/dependency freezes match. Final RED-API-CHECKPOINT.json and VALIDATION.md agree with direct evidence.

No blocker to scope-limited implementation following these observed contracts. No implementation GREEN, real five-family decoding, public normalization/rendering, or actual ignored-test acceptance. Both actual-family controls remain unrun. Preserve the earlier STOP and current limitations.

Machine-verifiable details: /tmp/tessera-owned-cfa-red-independent-review.json. Evidence roots are host-local /Volumes/betterSSD/tessera-validation/owned-captured-cfa/{243eacfe,c0993358}.
