.PHONY: run test-e2e bindings check-bindings

# Regenerate first: launching the app against stale bindings is how a broken
# `invoke` bridge stays invisible (`tsc` passes, the command fails at runtime).
run: bindings
	npm run tauri dev

test-e2e:
	npm run test:e2e -- $(ARGS)

# Regenerate the TypeScript wire types in src/bindings/ from the Rust
# definitions in crates/entry.
bindings:
	cargo test -p journal-entry --features ts

# Fail if the committed bindings are stale. Without this the generated types
# silently drift from the Rust ones and `tsc` stops catching bridge mismatches.
check-bindings:
	@before=$$(cat src/bindings/*.ts 2>/dev/null | sha256sum); \
	 $(MAKE) --no-print-directory bindings >/dev/null; \
	 after=$$(cat src/bindings/*.ts 2>/dev/null | sha256sum); \
	 test "$$before" = "$$after" \
	   || { echo "src/bindings was stale; regenerated — commit the result."; exit 1; }
