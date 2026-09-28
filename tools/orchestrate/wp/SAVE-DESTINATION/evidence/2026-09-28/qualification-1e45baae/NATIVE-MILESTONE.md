# Combined Save As native qualification

HEAD 5f34221d4dcd64a93a6de596e90c487d3ea31ffc. All gates Release, locked, jobs 2, deployment target 15.0, shared target recorded in command.json.

- 01-native-commit-01: direct exit 0; elapsed 239.49s; fixture unchanged=True; HEAD unchanged=True; FFI unchanged=True.
- 02-native-session-01: direct exit 0; elapsed 63.20s; fixture unchanged=True; HEAD unchanged=True; FFI unchanged=True.
- 03-native-roundtrip-01: direct exit 0; elapsed 8.34s; fixture unchanged=True; HEAD unchanged=True; FFI unchanged=True.
- 04-psd-psb-01: direct exit 0; elapsed 1.53s; fixture unchanged=True; HEAD unchanged=True; FFI unchanged=True.
- 05-native-document-01: direct exit 0; elapsed 3.71s; fixture unchanged=True; HEAD unchanged=True; FFI unchanged=True.
- 06-native-ffi-01: direct exit 0; elapsed 6.33s; fixture unchanged=True; HEAD unchanged=True; FFI unchanged=True.
- 07-native-strict-01: direct exit 0; elapsed 21.80s; fixture unchanged=True; HEAD unchanged=True; FFI unchanged=True.
- 08-native-format-01: direct exit 0; elapsed 6.84s; fixture unchanged=True; HEAD unchanged=True; FFI unchanged=True.
- 09-regenerate-01: direct exit 0; elapsed 68.00s; fixture unchanged=True; HEAD unchanged=True; FFI unchanged=False.

Counts: 01 publication 4 passed; 02 public session 5 passed; 03 roundtrip 1 passed; 04 PSD 1 passed; 05 document 11 passed/1 ignored; 06 FFI lib 193 passed/1 ignored. Gates 07 strict Clippy and 08 format passed. Ignored tests are not accepted as executed.

09 regenerated archive and bindings together. Reviewed generated diff: header +11, Swift +164, exclusively additive checked Save As method, typed result/intent enums and checksum 46144. No removals or changes to existing Smart Preview APIs/checksums. Module map unchanged. Source-change allowlist and fixture checks passed; archive change is expected only for this generation gate. Original inherited FFI files remain preserved in inherited-ffi.

Swift qualification remains pending; no GUI acceptance claim.
