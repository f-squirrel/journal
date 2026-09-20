#!/bin/sh
# Line coverage of the two reports `make coverage` writes, of both together,
# and of the lines this change touched.
#
# Scored the way Codecov scores an lcov upload, so the total sits close to the
# number on the README badge: from each `DA:` line record, not the `LF:`/`LH:`
# totals (llvm-cov's disagree with its own `DA:` lines), and a line whose
# `BRDA:` branches did not all run is partial, which counts as not covered.
# The total is (lines covered) / (lines found) over both files, as Codecov adds
# up a commit that has both uploaded.
#
# Output, in order:
#   rust / web / total   the percentages. The total also says how it moved
#                        since the last run here (`coverage/last-total`, which
#                        this script rewrites) and flags a total under the
#                        floor. That is a local signal, not Codecov's no-drop
#                        check, which compares against the base commit.
#   patch                coverage of the changed lines that a report has a `DA:`
#                        record for (comments, blanks and test modules have
#                        none), then the ones no test executed, per file as
#                        ranges, and changed web source files the web report does
#                        not list at all (a hook or component no e2e spec loads).
#
# The changed lines are the working tree against COV_BASE (default HEAD), plus
# every line of an untracked file. Set COV_BASE to the commit a feature started
# from when the work is already committed. COV_FLOOR (default 85) is the number
# in codecov.yml; keep them in sync. This reports and exits 0, CI gates.
set -eu

dir=${1:-coverage}
base=${COV_BASE:-HEAD}
floor=${COV_FLOOR:-85}

for report in rust web; do
  if [ ! -s "$dir/$report.lcov" ]; then
    echo "coverage-summary: $dir/$report.lcov is missing or empty; run 'make coverage' first" >&2
    exit 1
  fi
done

ranges=$(mktemp)
diff=$(mktemp)
trap 'rm -f "$ranges" "$diff"' EXIT

# `+++ ` lines only count before a file's first hunk, so an added line that
# starts with "++ " is not taken for a header. Unquoted paths, whatever the
# user's git config says about prefixes.
if ! git -c core.quotePath=false diff -U0 --no-renames --no-color --no-ext-diff \
  --src-prefix=a/ --dst-prefix=b/ "$base" -- >"$diff"; then
  echo "coverage-summary: cannot diff against '$base' (set COV_BASE to a commit)" >&2
  exit 1
fi

# One `path:first:last` line per run of added lines.
awk '
  /^diff --git / { header = 1; path = "" }
  header && /^\+\+\+ / {
    path = substr($0, 5)
    if (path == "/dev/null") path = ""; else sub("^b/", "", path)
  }
  /^@@ / {
    header = 0
    if (path == "") next
    plus = $3                       # +start[,count]
    sub("^\\+", "", plus)
    n = split(plus, p, ",")
    count = (n > 1) ? p[2] : 1
    if (count > 0) print path ":" p[1] ":" (p[1] + count - 1)
  }
' "$diff" >"$ranges"
git -c core.quotePath=false ls-files --others --exclude-standard |
  awk '{ print $0 ":1:1000000000" }' >>"$ranges"

awk -F'[:,]' -v ranges="$ranges" -v floor="$floor" -v last="$dir/last-total" '
  # A changed web source file no report lists was never loaded by a test. Only
  # the web report is checked: llvm-cov lists every Rust file that has
  # executable code, at 0% if no test runs it, so a Rust file it leaves out
  # (`lib.rs` of mod declarations, traits, error enums, test support) has
  # nothing to cover, and flagging it would be a false alarm each time it is
  # edited. Web files with no statements or that vite.config.ts excludes from
  # instrumentation are likewise not reported: generated bindings, the test
  # fallback, declaration files, and `types.ts`.
  function is_source(p) {
    return p ~ /^src\/.*\.tsx?$/ && p !~ /^src\/api\/fallback\.ts$/ \
      && p !~ /^src\/bindings\// && p !~ /\.d\.ts$/ && p !~ /^src\/types\.ts$/
  }
  function is_changed(p, l,   i) {
    for (i = 1; i <= nr[p]; i++) if (l >= rs[p, i] && l <= re[p, i]) return 1
    return 0
  }
  function add_run() {
    if (run != "") run = run ","
    run = run (from == to ? from : from "-" to)
  }

  BEGIN {
    if ((getline prev < last) > 0 && prev ~ /^[0-9]+(\.[0-9]+)?$/) have_last = 1
    close(last)
  }

  FILENAME == ranges {
    nr[$1]++
    rs[$1, nr[$1]] = $2; re[$1, nr[$1]] = $3
    if (!($1 in is_listed)) { is_listed[$1] = 1; changed_files[++ncf] = $1 }
    next
  }

  FNR == 1 { name = FILENAME; sub(".*/", "", name); sub("\\.lcov$", "", name) }
  /^SF:/ { path = $2; seen[path] = 1; maxl = 0 }
  /^DA:/ { line[$2] = ($3 > 0); if ($2 + 0 > maxl) maxl = $2 + 0 }
  /^BRDA:/ { if ($5 == "-" || $5 == 0) partial[$2] = 1 }
  /^end_of_record/ {
    for (l in line) {
      found[name]++
      if (line[l] && !(l in partial)) hit[name]++
    }
    if (path in nr) {
      run = ""; from = to = 0
      for (l = 1; l <= maxl; l++) {
        if (!(l in line) || !is_changed(path, l)) continue
        pfound++
        if (line[l] && !(l in partial)) { phit++; continue }
        if (to != 0 && l == to + 1) { to = l; continue }
        if (to != 0) add_run()
        from = to = l
      }
      if (to != 0) add_run()
      if (run != "") entries[++nent] = "  " path ": " run
    }
    delete line; delete partial
  }
  END {
    n = split("rust web", names, " ")
    for (i = 1; i <= n; i++) {
      k = names[i]
      if (found[k] == 0) {
        print "coverage-summary: " k ".lcov lists no lines" > "/dev/stderr"
        exit 1
      }
      printf "%-6s %5.1f%%  (%d/%d lines)\n", k, 100 * hit[k] / found[k], hit[k], found[k]
      total_hit += hit[k]; total_found += found[k]
    }

    pct = 100 * total_hit / total_found
    now10 = int(pct * 10 + 0.5)
    printf "%-6s %5.1f%%  (%d/%d lines)", "total", pct, total_hit, total_found
    if (have_last) {
      d = now10 - int(prev * 10 + 0.5)
      if (d == 0) printf "  +0.0 vs last run"
      else printf "  %+.1f vs last run", d / 10
    } else {
      printf "  (first run)"
    }
    if (pct < floor) printf " BELOW %s%% FLOOR", floor
    printf "\n"
    printf "%.1f\n", now10 / 10 > last
    close(last)

    if (pfound > 0) printf "%-6s %5.1f%%  (%d/%d changed lines)\n", "patch", 100 * phit / pfound, phit, pfound
    else print "patch   n/a  (no changed lines in the reports)"

    # A changed source file in no report at all: never loaded by a test.
    for (i = 1; i <= ncf; i++) {
      p = changed_files[i]
      if (!(p in seen) && is_source(p))
        entries[++nent] = "  " p ": not in the report (never loaded by a test?)"
    }
    if (nent > 0) {
      print "uncovered, changed:"
      for (i = 1; i <= nent && i <= 10; i++) print entries[i]
      if (nent > 10) printf "  ... and %d more files\n", nent - 10
    }
  }
' "$ranges" "$dir/rust.lcov" "$dir/web.lcov"
