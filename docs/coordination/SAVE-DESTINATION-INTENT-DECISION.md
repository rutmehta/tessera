# Save As destination intent: bounded implementation decision

The additive design f426c9de is accepted for planning with two required corrections from independent review; its proposed hard-link-only path is not approved. Source only; no implementation/runtime acceptance.

Use pinned tempfile::persist_noclobber for CreateIfAbsent so macOS exclusive rename remains available before the library hard-link fallback. Preserve atomic no-overwrite behavior and fail on unsupported commit rather than replacing. The library fallback may ignore staging unlink failure; report this honestly as cleanup observability, not a guarantee of complete cleanup. Do not delete a reused or unowned path in an attempted cleanup workaround. A successful destination publication is Saved even if later cleanup fails; diagnostics must not misreport untouched destination or enable destructive retries.

Retain existing explicitly confirmed Replace destination-path semantics and legacy calls. No identity-CAS requirement or disabled Replace. Native unique staging removes the current deterministic temporary-name collision for document-session output; separate copy APIs remain unchanged.

Stub save serialization must take the save gate before reading the current destination path, then snapshot under its model lock in that ordering. Otherwise a concurrent Save As can make ordinary Save write a stale path. Record the captured saved head after publication so later edits remain dirty.

Implementation order: A native helper/API and tiny commit/marker tests in a separate source branch; B backend/stub/UI follows an actual generated API checkpoint and accepted presenter baseline. Current owned-presenter validation uses frozen current4a45 native archive and must not acquire this future work silently. No extra B workloads, no main merge without bounded gates and UI integration.
