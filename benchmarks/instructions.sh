#!/usr/bin/env bash
# Instructions and branch mispredictions per tick, one agent, under cachegrind.
# Deterministic, so it is steadier than wall-clock time on a shared machine.
# Runs N and 2N ticks and divides the difference by N, cancelling setup.
set -euo pipefail
cd "$(dirname "$0")"
N=${N:-200000}
LIBS=${LIBS:-"flatbt bonsai-bt behavior-tree bhv behavior-tree-lite"}
SCENARIOS=${SCENARIOS:-"select8 patrol guard soldier"}
cargo build --release --quiet
BIN=target/release/flatbt-compare

count() { # lib scenario ticks -> "instructions mispredicts"
  valgrind --tool=cachegrind --cache-sim=no --branch-sim=yes --cachegrind-out-file=/dev/null \
    "$BIN" ticks "$1" "$2" "$3" 2>&1 >/dev/null |
    awk '/I *refs:/ {gsub(",", "", $4); i=$4} /Mispredicts:/ {gsub(",", "", $3); m=$3} END {print i, m}'
}

for s in $SCENARIOS; do
  echo
  echo "### $s"
  echo
  echo "| library | instructions/tick | branch mispredicts/tick |"
  echo "|---|--:|--:|"
  for l in $LIBS; do
    read -r i1 m1 < <(count "$l" "$s" "$N")
    read -r i2 m2 < <(count "$l" "$s" $((2 * N)))
    awk -v l="$l" -v n="$N" -v i1="$i1" -v i2="$i2" -v m1="$m1" -v m2="$m2" \
      'BEGIN { printf "| %s | %.1f | %.2f |\n", l, (i2 - i1) / n, (m2 - m1) / n }'
  done
done
