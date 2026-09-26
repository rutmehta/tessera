.PHONY: bench
# From the repository root: make -f tools/bench/Makefile bench
bench:
	@test -n "$(CARGO_TARGET_DIR)" || (printf '%s\n' 'Set CARGO_TARGET_DIR outside the checkout'; exit 2)
	@unset DYLD_FALLBACK_LIBRARY_PATH; cargo run --locked --release -p tessera-bench -- $(BENCH_ARGS)
