#!/bin/sh
# Line coverage of the two reports `make coverage` writes, and of both together.
#
# The total is (lines hit) / (lines found) summed over both lcov files, which
# is how Codecov adds up a commit that has both uploaded, so it should sit
# close to the number on the README badge. Only the `LF:` and `LH:` totals of
# each file are read.
set -eu

dir=${1:-coverage}

for report in rust web; do
  if [ ! -s "$dir/$report.lcov" ]; then
    echo "coverage-summary: $dir/$report.lcov is missing or empty; run 'make coverage' first" >&2
    exit 1
  fi
done

awk -F: '
  FNR == 1 { name = FILENAME; sub(".*/", "", name); sub("\\.lcov$", "", name) }
  /^LF:/ { lf[name] += $2; total_lf += $2 }
  /^LH:/ { lh[name] += $2; total_lh += $2 }
  END {
    n = split("rust web", names, " ")
    for (i = 1; i <= n; i++) {
      k = names[i]
      if (lf[k] == 0) {
        print "coverage-summary: " k ".lcov lists no lines" > "/dev/stderr"
        exit 1
      }
      printf "%-6s %5.1f%%  (%d/%d lines)\n", k, 100 * lh[k] / lf[k], lh[k], lf[k]
    }
    printf "%-6s %5.1f%%  (%d/%d lines)\n", "total", 100 * total_lh / total_lf, total_lh, total_lf
  }
' "$dir/rust.lcov" "$dir/web.lcov"
