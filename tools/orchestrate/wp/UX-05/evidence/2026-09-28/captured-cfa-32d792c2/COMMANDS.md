# Closed CFA decoder commands
Harness validates all five recorded fixture families/hash identities before every command, freezes tracked+new source, logs direct exits and rehashes fixtures afterward. No overwriting earlier run names.

python3 run.py 01-red cargo test -p raw-decode --release capture::tests::decode -- --nocapture
python3 run.py 02-green cargo test -p raw-decode --release capture::tests::decode -- --nocapture
python3 run.py 03-five-family cargo test -p raw-decode --release capture::tests::decode::actual_ -- --ignored --nocapture
python3 run.py 04-full cargo test -p raw-decode --release
python3 run.py 05-strict cargo clippy -p raw-decode --release --all-targets -- -D warnings
python3 run.py 06-fmt cargo fmt --all --check

Ordinary suite has two intentionally ignored qualification tests; only explicit03 can claim their execution. Fixture sources are read-only; actual decoder tests replace isolated copies only.
