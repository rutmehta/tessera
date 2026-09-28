# Actual parent loss during nested folder chooser

Test-only a9276e90 adds one real AppKit test to full/GUI-tested d695e6a1. Product Swift Sources and Rust/Cargo are identical; no product guard or lifecycle code changed. Separate test compilation succeeded, then selected --skip-build runtime executed one test in0.990s, zero failures/direct0/no watchdog. Read actual watchdog/process settings from the runner evidence; timeout was not used as proof.

The test attached a real ParentView and presented a real AppKitDocumentSaveSession form, entered actual NSOpenPanel.runModal, and scheduled only a captured parent.close action while the child was active. It observed modal entry, chooser return, suppressed outer selection callback, host loss once, real presenter drain once, captured sheet removed from both attachedSheet and parent.sheets, and idle presenter/cleared host. A bounded XCTest expectation waited for the actual drain event. No completion/detachment was synthesized.

Root verified recorded test/archive hashes after execution, clean a927 HEAD, and identical product sources to the previous full642/1skip/0+5 and GUI candidate. This is one additional native lifecycle check, not a claim the entire suite was rerun with643 cases. Probe-held native object deallocation/leak freedom is not claimed.
