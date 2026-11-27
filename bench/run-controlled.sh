#!/usr/bin/env bash
# Controlled Legion reference measurements. Disk-cold open is measured separately from
# process-first and warm operations; never label a just-created fixture as disk-cold.
set -euo pipefail

label=legion repeat=3 size=full underlay= drop=0 force=0
while [ $# -gt 0 ]; do
  case "$1" in
    --label) label=$2; shift 2 ;;
    --repeat) repeat=$2; shift 2 ;;
    --size) size=$2; shift 2 ;;
    --underlay) underlay=$(realpath -- "$2"); shift 2 ;;
    --drop-caches) drop=1; shift ;;
    --force) force=1; shift ;;
    *) echo "unknown option $1" >&2; exit 2 ;;
  esac
done
[[ "$repeat" =~ ^[1-9][0-9]*$ ]] || { echo "invalid repeat count" >&2; exit 2; }
[[ "$size" = full || "$size" = smoke ]] || { echo "invalid size" >&2; exit 2; }

cd "$(dirname "$0")/.."
commit=$(git rev-parse HEAD)
require_clean() {
  if [ "$(git rev-parse HEAD)" != "$commit" ] || [ -n "$(git status --porcelain --untracked-files=normal)" ]; then
    echo "benchmark requires the same clean committed revision throughout" >&2
    exit 1
  fi
}
require_clean
cargo build --release --locked -p ecad-bench
bench=${CARGO_TARGET_DIR:-target}/release/ecad-bench
out=bench/results/$(date -u +%Y%m%dT%H%M%SZ)-${commit:0:12}
mkdir -p "$out"
export ECAD_BENCH_MACHINE=$label
export ECAD_BENCH_CONTROLLED=qualified
if [ "$force" = 1 ] || [ "$drop" != 1 ] || [ "$size" != full ] || [ "$repeat" -lt 3 ]; then
  export ECAD_BENCH_CONTROLLED=exploratory
fi
if [ -n "$underlay" ]; then
  export ECAD_BENCH_ASSET_SHA256=$(sha256sum -- "$underlay" | cut -d' ' -f1)
else
  export ECAD_BENCH_ASSET_SHA256=generated
fi

conditions() {
  echo "== $1 $(date -u +%FT%TZ)"
  echo "commit:   $commit"
  echo "rustc:    $(rustc --version)"
  echo "kernel:   $(uname -srm)"
  grep -m1 'model name' /proc/cpuinfo || true
  echo "governor: $(cat /sys/devices/system/cpu/cpu*/cpufreq/scaling_governor 2>/dev/null | sort | uniq -c | xargs || echo unknown)"
  for s in /sys/class/power_supply/*; do
    [ -f "$s/type" ] && echo "power:    $(basename "$s") $(cat "$s/type") online=$(cat "$s/online" 2>/dev/null || echo -) status=$(cat "$s/status" 2>/dev/null || echo -)"
  done
  echo "load:     $(cat /proc/loadavg)"
  echo "memory:   $(grep -E 'MemTotal|MemAvailable' /proc/meminfo | xargs)"
  echo "asset:    $ECAD_BENCH_ASSET_SHA256"
  echo "qualification: $ECAD_BENCH_CONTROLLED"
  echo "display:  ${DISPLAY:-none} / ${WAYLAND_DISPLAY:-none}"
  echo "top:"; ps -eo pcpu,pmem,comm --sort=-pcpu | sed -n '1,6p'
}

wait_idle() {
  for _ in $(seq 30); do
    load=$(cut -d' ' -f1 /proc/loadavg)
    if awk -v l="$load" 'BEGIN { exit !(l < 0.5) }'; then return; fi
    if [ "$force" = 1 ]; then return; fi
    sleep 10
  done
  echo "machine not idle; stop concurrent builds, emulators and inference" >&2
  exit 1
}
args=(--size "$size")
[ -z "$underlay" ] || args+=(--underlay "$underlay")
fixtures=$(mktemp -d)
trap 'rm -rf -- "$fixtures"' EXIT

# All disk fixtures exist before eviction. Do not copy, hash, load or rewrite the
# fixture between eviction and the first timed read.
for p in conveyor reference synthetic; do
  "$bench" prepare-open "$p" "$fixtures/$p" "${args[@]}"
done
require_clean
wait_idle
conditions before > "$out/conditions.txt"
for n in $(seq "$repeat"); do
  if [ "$drop" = 1 ]; then
    for p in conveyor reference synthetic; do
      wait_idle
      conditions "before disk-cold $p $n" >> "$out/conditions.txt"
      sync
      echo 3 | sudo tee /proc/sys/vm/drop_caches > /dev/null
      "$bench" measure-open "$p" "$fixtures/$p" --size "$size" > "$out/disk-cold-$p-$n.json"
    done
  fi
  wait_idle
  conditions "before process suite $n" >> "$out/conditions.txt"
  "$bench" suite "${args[@]}" --out "$out/process-$n.json"
done
require_clean
if [ -n "$underlay" ] && [ "$(sha256sum -- "$underlay" | cut -d' ' -f1)" != "$ECAD_BENCH_ASSET_SHA256" ]; then
  echo "underlay changed during run" >&2
  exit 1
fi
conditions after >> "$out/conditions.txt"
printf '%s\n' "$commit" > "$out/COMPLETE"
echo "results in $out"
