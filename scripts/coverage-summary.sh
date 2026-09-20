#!/bin/sh
# Line coverage of the two reports `make coverage` writes, and of both together.
#
# Scored the way Codecov scores an lcov upload, so the total sits close to the
# number on the README badge: from each `DA:` line record, not the `LF:`/`LH:`
# totals (llvm-cov's disagree with its own `DA:` lines), and a line whose
# `BRDA:` branches did not all run is partial, which counts as not covered.
# The total is (lines covered) / (lines found) over both files, as Codecov adds
# up a commit that has both uploaded.
set -eu

dir=${1:-coverage}

for report in rust web; do
  if [ ! -s "$dir/$report.lcov" ]; then
    echo "coverage-summary: $dir/$report.lcov is missing or empty; run 'make coverage' first" >&2
    exit 1
  fi
done

awk -F'[:,]' '
  FNR == 1 { name = FILENAME; sub(".*/", "", name); sub("\\.lcov$", "", name) }
  /^DA:/ { line[$2] = ($3 > 0) }
  /^BRDA:/ { if ($5 == "-" || $5 == 0) partial[$2] = 1 }
  /^end_of_record/ {
    for (l in line) {
      found[name]++
      if (line[l] && !(l in partial)) hit[name]++
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
    printf "%-6s %5.1f%%  (%d/%d lines)\n", "total", 100 * total_hit / total_found, total_hit, total_found
  }
' "$dir/rust.lcov" "$dir/web.lcov"
