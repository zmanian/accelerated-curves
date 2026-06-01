# Accelerated Curves

Experimental Pasta-curve acceleration workspace for Zcash-adjacent proving and
verification code. The current work adds a conservative acceleration facade and
threads it through Ragu, Halo2, and Zebra without changing default consensus
behavior.

The safety posture is simple:

- CPU remains the default backend.
- CUDA and AVX-512 are opt-in.
- CUDA must report real runtime availability before it is treated as usable.
- AVX-512 currently exposes runtime detection only; it is not a usable MSM
  backend yet.
- Zebra crosscheck mode remains CPU-accepting, and experimental accept is
  behind explicit feature/config gates plus backend self-tests.

## Workspace Layout

- `crates/zcash-pasta-accel`
  - shared Rust acceleration facade for Pallas/Vesta MSM dispatch
  - CPU implementation, backend selection, scoped dispatch config, stats, and
    checked FFI point/field layouts
- `repos/pasta-msm`
  - personal branch: `codex/pasta-msm-fallible`
  - fallible `try_pallas` and `try_vesta` APIs plus CUDA runtime availability
    probing
- `repos/ragu`
  - personal branch: `codex/accel-msm`
  - feature-gated `accel-msm` integration and PCD acceleration plumbing
- `repos/halo2`
  - personal branch: `codex/pasta-accel-dispatch`
  - feature-gated `pasta-accel` dispatch for large `best_multiexp` calls
- `repos/zebra`
  - personal branch: `codex/halo2-accel-verify`
  - Halo2 acceleration config parsing, batch metrics, CPU-protected crosscheck
    paths, offline replay scaffolding, and fuzz/regression coverage for
    invalid Orchard auth data

Reference clones such as Orchard, `pasta_curves`, Tachyon, and `sppark` are
inspection baselines for the current slice; they are not edited for the MVP.

## Clone Setup

The root crate has an optional path dependency on `repos/pasta-msm`, so clone
that adapter before running Cargo commands from the root workspace.

```sh
git clone --branch codex/accelerated-curves https://github.com/zmanian/accelerated-curves.git
cd accelerated-curves

mkdir -p repos
git clone --branch codex/pasta-msm-fallible https://github.com/zmanian/pasta-msm.git repos/pasta-msm
git clone --branch codex/accel-msm https://github.com/zmanian/ragu.git repos/ragu
git clone --branch codex/pasta-accel-dispatch https://github.com/zmanian/halo2.git repos/halo2
git clone --branch codex/halo2-accel-verify https://github.com/zmanian/zebra.git repos/zebra
```

## Local Verification

Use local commands as the main development loop. GitHub Actions exists as a
backup signal, but it should not block ordinary iteration.

The focused local runner wraps the common root and clone checks:

```sh
scripts/local-check.sh list
scripts/local-check.sh root-fast
scripts/local-check.sh ragu
scripts/local-check.sh ragu-fft
scripts/local-check.sh halo2
scripts/local-check.sh zebra
scripts/local-check.sh zebra-fuzz
```

Root facade:

```sh
cargo fmt --all --check
cargo test --workspace
cargo test --workspace --no-default-features
cargo clippy --workspace --all-targets -- -D warnings
cargo test -p zcash-pasta-accel --features cuda -- --test-threads=1
cargo test -p zcash-pasta-accel --features avx512 -- --test-threads=1
cargo check --manifest-path crates/zcash-pasta-accel/fuzz/Cargo.toml --bin msm_inputs
```

Ragu:

```sh
cd repos/ragu
cargo test -p ragu_arithmetic --features accel-msm -- --test-threads=1
cargo test -p ragu_pcd --features accel-msm seed_with_ -- --test-threads=1
cargo test -p ragu_arithmetic --features accel-fft accel_fft_stats -- --test-threads=1
cargo bench -p ragu_arithmetic --features accel-fft --bench fft_criterion --no-run
cargo bench -p ragu_pcd --features accel-msm,accel-fft --bench pcd_accel_criterion --no-run
```

Halo2:

```sh
cd repos/halo2
cargo test -p halo2_proofs --features pasta-accel test_pasta_accel -- --test-threads=1
cargo clippy -p halo2_proofs --features pasta-accel --all-targets -- -D warnings
cargo bench -p halo2_proofs --features pasta-accel --bench msm --no-run
```

Zebra:

```sh
cd repos/zebra
cargo test -p zebra-consensus --features halo2-accel-verify config::tests::halo2_accel -- --test-threads=1
cargo test -p zebra-consensus --features halo2-accel-verify halo2_batch_accel_context -- --test-threads=1
cargo clippy -p zebra-consensus --features halo2-accel-verify --all-targets -- -D warnings
cargo check --manifest-path zebra-consensus/fuzz/Cargo.toml --bin halo2_batch_items
cargo check --manifest-path zebra-consensus/fuzz/Cargo.toml --bin halo2_invalid_proofs
cargo clippy --manifest-path zebra-consensus/fuzz/Cargo.toml --bin halo2_invalid_proofs --features halo2-accel-verify -- -D warnings
```

Ragu acceleration fuzz target:

```sh
cd repos/ragu
cargo +nightly fuzz check --fuzz-dir qa/fuzz --features accel-msm fuzz_accelerated_commitments
```

## Runtime Knobs

- `ZCASH_ACCEL=off|cpu|cuda|avx512|auto`
- `ZCASH_ACCEL_MIN_MSM=<usize>`
- `ZCASH_ACCEL_VERIFY_MODE=cpu|crosscheck|experimental-accept`

Integrations can also use `zcash_pasta_accel::with_dispatch_config` for scoped
runtime selection without mutating process-wide environment variables.

## Hardware Validation

Local CPU checks cover dispatch, fallback, crosscheck, fuzz-target compile/lint,
and safety gates. Production-strength CUDA and AVX-512 evidence still requires
cloud or bare-metal hosts with the relevant hardware.

When those hosts are available, use the focused commands in
`docs/hardware-validation.md`. Capture host evidence, backend availability,
facade tests, Ragu/Halo2/Zebra focused tests, and benchmark output. Treat those
runs as hardware burn-in evidence, not as a replacement for the local loop.

## Documentation

- `ACCEL_NOTES.md` is the long-form implementation log and evidence ledger.
- `docs/acceleration.md` covers feature flags, environment variables,
  verification commands, and current limitations.
- `docs/acceleration-security.md` records consensus-safety rules.
- `docs/ragu-prover-accel.md` covers Ragu prover integration and benchmark
  gaps.
- `docs/zebra-halo2-accel.md` covers Zebra config, metrics, replay benchmarks,
  and fuzz targets.
- `docs/hardware-validation.md` covers CUDA and AVX-512 host validation.

## Current Open Work

- Run CUDA and AVX-512 validation on real hardware.
- Add Zebra experimental-accept replay coverage after hardware burn-in evidence
  exists.
- Replace repeated local replay bundles with historical or synthetic valid
  Orchard corpora when available.
- Add real AVX-512 MSM implementation before treating `Backend::Avx512` as a
  usable backend.
