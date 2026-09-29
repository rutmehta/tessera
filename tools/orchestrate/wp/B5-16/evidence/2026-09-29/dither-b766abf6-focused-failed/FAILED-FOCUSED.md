# b766 focused gate stopped on native AX assertions

Directexit1;60 tests executed,59 cases passed, one case failed with3 assertions. Source/HEAD/fourFFI/Sony/runner/import+oracle+baseline all frozen equal. Build completed93.73s; gate104.19s. No skips. Existing warnings retained. Layout,strict and GUI are UNRUN. Runtime explicitly released; no source edits/retries. Failed app/test executables copied and hashed in FAILED-FOCUSED.json.

DocumentDitherCheckboxTests.testNativeCheckboxMetadataValueAndNativeFocusPolicy:

- line21: detached box.accessibilityRole() returns AXUnknown, expectedAXCheckBox.
- line24: detached box.accessibilityValue() returnsnil, expected1.
- line34: same direct value accessor returnsnil after disabled/off configuration, expected0.

Five other new cases and prior54 pass, including actual editor adapter installation/model edit/Undo, native Space callbacks/repeat suppression, update/disabled/teardown behavior, lifetime and key partition. That does not establish exposed AX correctness or actual Tab behavior.

## Source-only diagnosis checkpoint

The failed test constructs a detached NSButton subclass with .switch type and queries the view directly. Its stock NSButton comparator is used only for acceptsFirstResponder, not AX role/value. The hosted integration test verifies adapter installation and editing, not its exposed AX element. NSApplication already exists because the hosted test precedes this case; lack of NSApplication creation is not supported as the root cause.

Root cause is **not yet proved**: direct accessor on detached view may differ from the actually exposed native accessibility element; alternatively the subclass may genuinely need accessibility semantics. Smallest discriminating next diagnostic: compare stock NSButton(checkboxWithTitle:) and adapter under identical detached and hosted conditions, record direct view role/value plus actual accessible children/unignored checkbox element, and retain semantic checkbox role/value assertions on the authoritative exposed element. Use bounded hosted window+autoreleasepool/cleanup, no forcedfocus/globalprefs. If stock and adapter differ, investigate product; if both direct accessors lack role/value while their exposed native elements are correct, fix test setup/oracle accordingly. Do not hardcode AXUnknown/nil as accepted, remove semantic assertions, or add explicit product role/value merely to satisfy an unproven detached oracle. B owns correction after evidence/source review.
