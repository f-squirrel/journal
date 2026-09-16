.PHONY: run build test rust-test test-e2e bindings check-bindings

# Regenerate first: launching the app against stale bindings is how a broken
# `invoke` bridge stays invisible (`tsc` passes, the command fails at runtime).
run: bindings
	npm run tauri dev

# Build the native installers. Checks rather than regenerates: a release
# build should fail on binding drift, not quietly paper over it.
build: check-bindings
	npm run tauri build

# The whole suite: the Rust unit tests first, since they are the faster
# feedback and the e2e run has to start a dev server.
test: rust-test test-e2e

rust-test:
	cargo test

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
