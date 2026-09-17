.PHONY: run build test rust-test test-e2e bindings check-bindings check-no-fallback

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

# Fail if the browser fallback reached a production bundle. The fallback
# fabricates two entries and accepts saves into a Map that dies with the
# page, so a shipped build that served it would report writes that never
# happened. It is `import.meta.env.DEV`-gated in src/api/entries.ts, but
# that gate is only as good as the bundler dropping the chunk, which is
# what this checks. Greps for the seed data and the fault hook separately:
# they can be retained independently.
check-no-fallback:
	npm run build
	@! grep -rq "First day back" dist/assets/*.js \
	  || { echo "the seeded fallback shipped in a production bundle"; exit 1; }
	@! grep -rq "failMessage" dist/assets/*.js \
	  || { echo "the fault-injection hook shipped in a production bundle"; exit 1; }
