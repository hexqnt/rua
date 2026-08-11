#!/usr/bin/env bash

# Сборка и perf-профилирование основной нагрузки RUA.

set -Eeuo pipefail

readonly PROJECT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
readonly PROFILE_DIR="${PROFILE_DIR:-$PROJECT_DIR/out/profiles}"
readonly DEFAULT_PERF_EVENTS="task-clock,context-switches,cpu-migrations,page-faults,\
cycles:u,instructions:u,branches:u,branch-misses:u,cache-references:u,cache-misses:u"
readonly PERF_EVENTS="${PERF_EVENTS:-$DEFAULT_PERF_EVENTS}"
readonly PERF_STAT_REPEATS="${PERF_STAT_REPEATS:-3}"
readonly PERF_RECORD_FREQUENCY="${PERF_RECORD_FREQUENCY:-499}"

config="config.toml"
build_only=false
profile_mode="all"

usage() {
    cat <<'EOF'
Usage:
  ./profiling.sh [options]

Options:
  -c, --config <path>  Configuration file (default: config.toml)
      --build-only     Build the profiling binary without running perf
      --record-only    Record call stacks without collecting counters
      --stat-only      Collect counters without recording call stacks
  -h, --help           Show this help

Environment:
  PROFILE_DIR              Output directory (default: out/profiles)
  PERF_STAT_REPEATS        perf stat repeat count (default: 3)
  PERF_RECORD_FREQUENCY    perf record sampling frequency (default: 499)
  PERF_EVENTS              Comma-separated perf stat events

Examples:
  ./profiling.sh
  PERF_STAT_REPEATS=5 ./profiling.sh --config config.toml
  perf report --input out/profiles/rua.perf.data
EOF
}

require_option_value() {
    local option="$1"
    local value="${2-}"
    if [[ -z "$value" ]]; then
        echo "error: $option requires a value" >&2
        usage >&2
        exit 2
    fi
}

select_profile_mode() {
    local requested_mode="$1"
    local option="$2"
    if [[ "$profile_mode" != "all" ]]; then
        echo "error: $option cannot be combined with another profiling mode" >&2
        exit 2
    fi
    profile_mode="$requested_mode"
}

while (($# > 0)); do
    case "$1" in
        -c|--config)
            require_option_value "$1" "${2-}"
            config="$2"
            shift 2
            ;;
        --build-only)
            build_only=true
            shift
            ;;
        --record-only)
            select_profile_mode "record" "$1"
            shift
            ;;
        --stat-only)
            select_profile_mode "stat" "$1"
            shift
            ;;
        -h|--help)
            usage
            exit 0
            ;;
        *)
            echo "error: unknown option: $1" >&2
            usage >&2
            exit 2
            ;;
    esac
done

if [[ "$build_only" == true && "$profile_mode" != "all" ]]; then
    echo "error: --build-only cannot be combined with a profiling mode" >&2
    exit 2
fi
if [[ "$build_only" == false && "$profile_mode" != "record" \
    && ! "$PERF_STAT_REPEATS" =~ ^[1-9][0-9]*$ ]]; then
    echo "error: PERF_STAT_REPEATS must be a positive integer" >&2
    exit 2
fi
if [[ "$build_only" == false && "$profile_mode" != "stat" \
    && ! "$PERF_RECORD_FREQUENCY" =~ ^[1-9][0-9]*$ ]]; then
    echo "error: PERF_RECORD_FREQUENCY must be a positive integer" >&2
    exit 2
fi

cd -- "$PROJECT_DIR"
if [[ ! -f "$config" ]]; then
    echo "error: config does not exist: $config" >&2
    exit 2
fi
if [[ "$build_only" == false ]] && ! command -v perf >/dev/null 2>&1; then
    echo "error: perf is not installed or is not in PATH" >&2
    exit 1
fi

target_dir="${CARGO_TARGET_DIR:-$PROJECT_DIR/target}"
if [[ "$target_dir" != /* ]]; then
    target_dir="$PROJECT_DIR/$target_dir"
fi
readonly BINARY="$target_dir/profiling/rua"
readonly PERF_DATA="$PROFILE_DIR/rua.perf.data"
readonly PERF_STAT="$PROFILE_DIR/rua.perf-stat.txt"
readonly PERF_REPORT="$PROFILE_DIR/rua.perf-report.txt"

profiling_rustflags="-C force-frame-pointers=yes"
if [[ -n "${RUSTFLAGS-}" ]]; then
    profiling_rustflags="$RUSTFLAGS $profiling_rustflags"
fi

echo "Building optimized profiling binary"
env "RUSTFLAGS=$profiling_rustflags" cargo build --profile profiling --bin rua
if [[ ! -x "$BINARY" ]]; then
    echo "error: Cargo did not produce executable $BINARY" >&2
    exit 1
fi
if [[ "$build_only" == true ]]; then
    echo "Built: $BINARY"
    exit 0
fi

mkdir -p -- "$PROFILE_DIR"
declare -ar workload=("$BINARY" --config "$config")

if [[ "$profile_mode" != "stat" ]]; then
    echo "Recording call stacks"
    perf record \
        --quiet \
        --freq "$PERF_RECORD_FREQUENCY" \
        --event cycles:u \
        --call-graph fp \
        --output "$PERF_DATA" \
        -- "${workload[@]}"
    perf report \
        --stdio \
        --input "$PERF_DATA" \
        --no-children \
        --call-graph none \
        --sort dso,symbol \
        >"$PERF_REPORT"
fi

if [[ "$profile_mode" != "record" ]]; then
    echo "Collecting counters ($PERF_STAT_REPEATS repeats)"
    perf stat \
        --repeat "$PERF_STAT_REPEATS" \
        --event "$PERF_EVENTS" \
        --output "$PERF_STAT" \
        -- "${workload[@]}"
fi

echo "Profiles written to: $PROFILE_DIR"
