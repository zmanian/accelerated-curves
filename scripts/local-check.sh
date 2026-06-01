#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"

usage() {
  cat <<'USAGE'
Usage: scripts/local-check.sh <target>

Targets:
  list            Show this help.
  root-fast       Fast root facade checks.
  root            Full root workspace checks.
  ragu            Focused Ragu accel-msm tests.
  ragu-fuzz       Check the optional Ragu accelerated-commitments fuzz target.
  halo2           Focused Halo2 pasta-accel checks.
  zebra           Focused Zebra halo2-accel-verify checks.
  zebra-fuzz      Check and lint Zebra Halo2 fuzz targets.
  fuzz            Check root, Ragu, and Zebra fuzz targets.
  focused         Run root-fast, ragu, halo2, zebra, and zebra-fuzz.

The script intentionally runs local commands only. It does not dispatch or wait
on GitHub Actions.
USAGE
}

need_dir() {
  local path="$1"
  if [[ ! -d "$path" ]]; then
    echo "missing directory: $path" >&2
    echo "clone the integration repos first; see README.md Clone Setup" >&2
    exit 1
  fi
}

run() {
  local dir="$1"
  shift
  echo
  echo "+ (cd ${dir#$ROOT/} && $*)"
  (cd "$dir" && "$@")
}

root_fast() {
  run "$ROOT" cargo test -p zcash-pasta-accel -- --test-threads=1
  run "$ROOT" cargo check --manifest-path crates/zcash-pasta-accel/fuzz/Cargo.toml --bin msm_inputs
}

root_full() {
  run "$ROOT" cargo fmt --all --check
  run "$ROOT" cargo test --workspace
  run "$ROOT" cargo test --workspace --no-default-features
  run "$ROOT" cargo clippy --workspace --all-targets -- -D warnings
}

ragu_checks() {
  local dir="$ROOT/repos/ragu"
  need_dir "$dir"
  run "$dir" cargo test -p ragu_arithmetic --features accel-msm -- --test-threads=1
  run "$dir" cargo test -p ragu_pcd --features accel-msm seed_with_ -- --test-threads=1
}

ragu_fuzz_checks() {
  local dir="$ROOT/repos/ragu"
  need_dir "$dir"
  run "$dir" cargo +nightly fuzz check --fuzz-dir qa/fuzz --features accel-msm fuzz_accelerated_commitments
}

halo2_checks() {
  local dir="$ROOT/repos/halo2"
  need_dir "$dir"
  run "$dir" cargo test -p halo2_proofs --features pasta-accel test_pasta_accel -- --test-threads=1
  run "$dir" cargo clippy -p halo2_proofs --features pasta-accel --all-targets -- -D warnings
}

zebra_checks() {
  local dir="$ROOT/repos/zebra"
  need_dir "$dir"
  run "$dir" cargo test -p zebra-consensus --features halo2-accel-verify config::tests::halo2_accel -- --test-threads=1
  run "$dir" cargo test -p zebra-consensus --features halo2-accel-verify halo2_batch_accel_context -- --test-threads=1
}

zebra_fuzz_checks() {
  local dir="$ROOT/repos/zebra"
  need_dir "$dir"
  run "$dir" cargo check --manifest-path zebra-consensus/fuzz/Cargo.toml --bin halo2_batch_items
  run "$dir" cargo check --manifest-path zebra-consensus/fuzz/Cargo.toml --bin halo2_invalid_proofs
  run "$dir" cargo clippy --manifest-path zebra-consensus/fuzz/Cargo.toml --bin halo2_batch_items --features halo2-accel-verify -- -D warnings
  run "$dir" cargo clippy --manifest-path zebra-consensus/fuzz/Cargo.toml --bin halo2_invalid_proofs --features halo2-accel-verify -- -D warnings
}

fuzz_checks() {
  run "$ROOT" cargo check --manifest-path crates/zcash-pasta-accel/fuzz/Cargo.toml --bin msm_inputs
  ragu_fuzz_checks
  zebra_fuzz_checks
}

focused_checks() {
  root_fast
  ragu_checks
  halo2_checks
  zebra_checks
  zebra_fuzz_checks
}

target="${1:-focused}"

case "$target" in
  list|help|--help|-h)
    usage
    ;;
  root-fast)
    root_fast
    ;;
  root)
    root_full
    ;;
  ragu)
    ragu_checks
    ;;
  ragu-fuzz)
    ragu_fuzz_checks
    ;;
  halo2)
    halo2_checks
    ;;
  zebra)
    zebra_checks
    ;;
  zebra-fuzz)
    zebra_fuzz_checks
    ;;
  fuzz)
    fuzz_checks
    ;;
  focused)
    focused_checks
    ;;
  *)
    echo "unknown target: $target" >&2
    usage >&2
    exit 2
    ;;
esac
