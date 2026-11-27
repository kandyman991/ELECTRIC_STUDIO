#!/usr/bin/env bash
# The controlled benchmark run, for the owner's machine (the Legion): wall clock, peak memory and
# the graphical workloads, cold and warm, under recorded idle conditions.
#
#   bench/run-controlled.sh [--label legion] [--repeat 3] [--size full] [--underlay drawing.dwg]
#                           [--drop-caches] [--force]
#
# Writes bench/results/<date>-<commit>/ (not committed unless it is to become a baseline):
#   conditions.txt   machine, kernel, governor, power, load, toolchain, before and after
#   cold.json        one suite run right after dropping the page cache (with --drop-caches)
#   warm-<n>.json    the repeated suite runs
# Compare a run with a baseline: target/release/ecad-bench compare warm-1.json <baseline>.
#
# --underlay puts a real drawing behind the synthetic project's first page; it is read locally
# and never committed. --force runs even when the machine is not idle.
set -euo pipefail

label=legion repeat=3 size=full underlay= drop=0 force=0
while [ $# -gt 0 ]; do
  case "$1" in
    --label) label=$2; shift 2 ;;
    --repeat) repeat=$2; shift 2 ;;
    --size) size=$2; shift 2 ;;
    --underlay) underlay=$2; shift 2 ;;
    --drop-caches) drop=1; shift ;;
    --force) force=1; shift ;;
    *) echo "unknown option $1" >&2; exit 2 ;;
  esac
done

cd "$(dirname "$0")/.."
cargo build --release --locked -p ecad-bench
bench=${CARGO_TARGET_DIR:-target}/release/ecad-bench
commit=$(git rev-parse --short HEAD)
out=bench/results/$(date -u +%Y%m%dT%H%M%SZ)-$commit
mkdir -p "$out"
export ECAD_BENCH_MACHINE=$label

conditions() {
  echo "== $1 $(date -u +%FT%TZ)"
  echo "commit:   $(git rev-parse HEAD)$(git diff --quiet || echo ' (uncommitted changes)')"
  echo "rustc:    $(rustc --version)"
  echo "kernel:   $(uname -srm)"
  grep -m1 'model name' /proc/cpuinfo || true
  echo "governor: $(cat /sys/devices/system/cpu/cpu*/cpufreq/scaling_governor 2>/dev/null | sort | uniq -c | xargs || echo unknown)"
  for s in /sys/class/power_supply/*; do
    [ -f "$s/type" ] && echo "power:    $(basename "$s") $(cat "$s/type") online=$(cat "$s/online" 2>/dev/null || echo -) status=$(cat "$s/status" 2>/dev/null || echo -)"
  done
  echo "load:     $(cat /proc/loadavg)"
  echo "memory:   $(grep -E 'MemTotal|MemAvailable' /proc/meminfo | xargs)"
  echo "top:"; ps -eo pcpu,pmem,comm --sort=-pcpu | head -6
}

# Idle: the 1-minute load average under 0.5, waited for up to five minutes.
for _ in $(seq 30); do
  load=$(cut -d' ' -f1 /proc/loadavg)
  if awk -v l="$load" 'BEGIN { exit !(l < 0.5) }'; then idle=1; break; fi
  idle=0; sleep 10
done
if [ "${idle:-0}" != 1 ] && [ "$force" != 1 ]; then
  echo "the machine is not idle (load $(cat /proc/loadavg)); close other programs or pass --force" >&2
  exit 1
fi

conditions before > "$out/conditions.txt"
args=(suite --size "$size")
[ -n "$underlay" ] && args+=(--underlay "$underlay")

if [ "$drop" = 1 ]; then
  sync
  echo 3 | sudo tee /proc/sys/vm/drop_caches > /dev/null
  "$bench" "${args[@]}" --out "$out/cold.json"
fi
for n in $(seq "$repeat"); do
  "$bench" "${args[@]}" --out "$out/warm-$n.json"
done
conditions after >> "$out/conditions.txt"
echo "results in $out"
