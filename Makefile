.PHONY: run build test rust-test test-e2e bindings check-bindings check-no-fallback format rust-format ts-format coverage-rust coverage-web coverage coverage-summary coverage-open

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
# definitions in crates/entry and crates/sync-status.
bindings:
	cargo test -p journal-entry -p journal-sync-status --features ts

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

# Format every crate in the workspace. --all makes that explicit rather than
# relying on the default (format the root package plus its path deps).
rust-format:
	cargo fmt --all

# Format every TypeScript/TSX file. src/bindings/ is excluded via
# .prettierignore since it's ts-rs's own generated output (see `bindings`
# above) — reformatting it would make `make check-bindings` fail forever.
ts-format:
	npm run format

format: rust-format ts-format

# Rust line coverage, as lcov (for Codecov) and HTML (for `coverage-open`),
# from ONE test run: `--no-report` collects the profile, and each `report`
# renders it. Needs nightly, because the test modules opt out of the report
# with `#[coverage(off)]`, which is unstable (see `[workspace.lints.rust]` in
# Cargo.toml); a plain `cargo test` stays on stable. The regex drops what is
# test or build support and so not the app's own code: `tests/`, `examples/`,
# `build.rs`, `MemoryRemote` (test support behind the `testing` feature) and
# the `main.rs` shim. `clean` first, so a stale profile cannot leak in. The
# lcov file's paths are made repo-relative, as Codecov expects.
# One-time setup: `cargo binstall cargo-llvm-cov` and
# `rustup component add llvm-tools-preview --toolchain nightly`.
COV_TOOLCHAIN ?= +nightly
COV_IGNORE := (^|/)(tests|examples|benches)/|(^|/)build\.rs$$|crates/remote/src/memory\.rs$$|src-tauri/src/main\.rs$$

coverage-rust:
	mkdir -p coverage
	cargo $(COV_TOOLCHAIN) llvm-cov clean --workspace
	cargo $(COV_TOOLCHAIN) llvm-cov --workspace --no-report
	cargo $(COV_TOOLCHAIN) llvm-cov report --lcov \
	  --output-path coverage/rust.lcov --ignore-filename-regex '$(COV_IGNORE)'
	sed 's#^SF:$(CURDIR)/#SF:#' coverage/rust.lcov > coverage/rust.lcov.tmp \
	  && mv coverage/rust.lcov.tmp coverage/rust.lcov
	cargo $(COV_TOOLCHAIN) llvm-cov report --html \
	  --output-dir coverage/rust-html --ignore-filename-regex '$(COV_IGNORE)'

# Front-end line coverage of the Playwright suite, as lcov (for Codecov) and
# HTML (for `coverage-open`). There are no front-end unit tests, so this is
# what the e2e run exercises against the browser fallback, not the Rust
# backend. `COVERAGE=1` makes the dev server instrument `src/` and
# `e2e/coverage.ts` save each page's counters into .nyc_output/, which the
# `nyc report` (configured in .nycrc.json) merges. Never instrument a build
# for `check-no-fallback`: coverage only ever runs against the dev server.
coverage-web:
	rm -rf .nyc_output coverage/web-html coverage/web.lcov
	mkdir -p coverage
	COVERAGE=1 npm run test:e2e
	npx nyc report
	cp coverage/web-html/lcov.info coverage/web.lcov

# Both reports, then the summary (`scripts/coverage-summary.sh`). Quiet: the
# two halves run in a sub-make with everything sent to $(COV_LOG), and only the
# summary reaches the terminal; a failed run prints the last 40 log lines and
# exits non-zero. `V=1` streams the full output instead. The halves are not
# prerequisites, and run one after the other, so `make -j coverage` cannot run
# cargo and Playwright at once.
# Nothing opens: safe to run in a script. `coverage-open` is the viewing half.
COV_LOG := coverage/run.log

coverage:
	@mkdir -p coverage
ifeq ($(V),1)
	@$(MAKE) --no-print-directory coverage-rust && $(MAKE) --no-print-directory coverage-web
else
	@{ $(MAKE) --no-print-directory coverage-rust && $(MAKE) --no-print-directory coverage-web; } >$(COV_LOG) 2>&1 \
	  || { echo "coverage run failed; last 40 lines of $(COV_LOG):"; tail -n 40 $(COV_LOG); exit 1; }
endif
	@$(MAKE) --no-print-directory coverage-summary

coverage-summary:
	@scripts/coverage-summary.sh

# Open the two HTML reports in the browser (`open` on macOS, `xdg-open`
# elsewhere; override with `make coverage-open OPEN=...`).
OPEN ?= $(shell if [ "$$(uname -s)" = Darwin ]; then echo open; else echo xdg-open; fi)
COV_REPORTS := coverage/rust-html/html/index.html coverage/web-html/index.html

coverage-open:
	@for report in $(COV_REPORTS); do \
	  test -f $$report || { echo "$$report is missing; run 'make coverage' first" >&2; exit 1; }; \
	done
	@for report in $(COV_REPORTS); do $(OPEN) $$report; done
