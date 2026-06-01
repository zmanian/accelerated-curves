# Ragu Prover Acceleration

Ragu is the first proving-side integration target. The current integration is
feature-gated and routes large Pasta variable-base MSMs through the shared
`zcash-pasta-accel` facade.

## Feature Flags

In `ragu_arithmetic`:

- `accel-msm`
- `accel-fft`

`accel-msm` enables the current dispatcher. `accel-fft` is reserved for future
FFT/NTT work and does not yet change FFT behavior.

## MSM Hook

Primary hook:

- `crates/ragu_arithmetic/src/util.rs`

The existing MSM implementation is preserved as `cpu_mul`. With `accel-msm`,
`mul` routes through `mul_with_accel_config`:

- collect contiguous scalar and base vectors
- check `ZCASH_ACCEL_MIN_MSM`
- call `zcash_pasta_accel::try_msm`
- fall back to `cpu_mul` on unsupported curves, backend errors, or accelerator
  misses

For callers that should not mutate process environment, `ragu_arithmetic`
also exposes:

- `AccelBackend`
- `AccelMsmConfig`
- `mul_with_accel_config`
- `AccelMsmStats`
- `accel_msm_stats`
- `reset_accel_msm_stats`

The stats counters track:

- candidate MSM count
- facade-result count
- fallback count
- total candidate points

Default threshold:

```sh
ZCASH_ACCEL_MIN_MSM=4096
```

## Commitment Hook

In `ragu_circuits`, enabling `accel-msm` also enables sparse polynomial
commitment helpers:

- `commit_with_accel_config`
- `commit_to_affine_with_accel_config`

The default `commit` and `commit_to_affine` APIs are unchanged. The explicit
helpers let prover-side call sites and tests pass an `AccelMsmConfig` directly,
including forced fallback cases such as `AccelBackend::Cuda` in a CPU-only
build.

## PCD Prover API

In `ragu_pcd`, enabling `accel-msm` exposes:

- `ProverAccelConfig`
- `Application::seed_with_accel_config`
- `Application::fuse_with_accel_config`
- `Application::rerandomize_with_accel_config`

`ProverAccelConfig` carries the requested backend, MSM threshold, future FFT
threshold, and a witness-buffer policy flag. The explicit APIs keep the default
PCD surface unchanged while allowing benchmark and burn-in callers to route real
proof construction commitments through the dispatcher without mutating process
environment.

For `seed_with_accel_config`, the synthetic trivial child proofs and the
subsequent fuse step are built with the same explicit config, so fallback
counters cover the whole seed construction.

The default `accel-msm` test pass now compiles broader explicit-config proof
path regressions for `fuse_with_accel_config` and
`rerandomize_with_accel_config`. Those two tests are marked ignored because
full PCD proof construction is too slow for the normal focused loop on a
laptop; run them explicitly when changing prover acceleration plumbing. Root CI
does run the focused `seed_with_accel_config_falls_back_and_verifies` proof-path
smoke test against the Ragu integration branch.

## Verification

Focused commands:

```sh
cargo test -p ragu_arithmetic
cargo test -p ragu_arithmetic --features accel-msm
cargo test -p ragu_arithmetic --features accel-msm test_accel_msm_records_forced_backend_fallback -- --test-threads=1
cargo test -p ragu_circuits --features accel-msm commit_with_accel_config_records_forced_backend_fallback -- --test-threads=1
cargo test -p ragu_circuits --features accel-msm commit_matches_dense -- --test-threads=1
cargo test -p ragu_pcd --features accel-msm with_accel_config_falls_back -- --test-threads=1
cargo test -p ragu_pcd --features accel-msm with_accel_config_falls_back -- --ignored --test-threads=1
cargo check -p ragu_arithmetic --no-default-features --features alloc
cargo check -p ragu_pcd --no-default-features --features alloc
cargo clippy -p ragu_arithmetic --features accel-msm --all-targets -- -D warnings
cargo clippy -p ragu_pcd --features accel-msm --all-targets -- -D warnings
cargo bench -p ragu_arithmetic --features accel-msm --bench msm_criterion --no-run
cargo bench -p ragu_arithmetic --features accel-msm --bench fft_criterion --no-run
```

## Benchmark Hooks

Current existing benchmark targets:

- `crates/ragu_arithmetic/benches/criterion/msm.rs`
- `crates/ragu_arithmetic/benches/criterion/fft.rs`
- `crates/ragu_pcd/benches/criterion/pcd_accel.rs`

When built with `accel-msm`, the MSM Criterion bench prints `AccelMsmStats`
after the benchmark group. This gives a lightweight view of candidate MSMs,
facade results, fallbacks, and total candidate points for the run.

The PCD Criterion benchmark covers:

- `Application::seed`
- `Application::fuse`
- `Application::rerandomize`
- `Application::verify`

It compares default CPU APIs with explicit `ProverAccelConfig` APIs under
`accel-msm`, including a forced CUDA fallback path for CPU-only machines. It
also prints `AccelMsmStats` after the benchmark group.

Target measurements still needed for production benchmark reports:

- wall-clock time
- backend used
- MSM count
- total MSM points
- FFT count
- total FFT domain size
- fallback count

## Remaining Work

- capture cloud-machine timings for the ignored full proof-path fallback
  regressions once AVX-512 and CUDA hosts are available
- route future FFT acceleration through `accel-fft` only after benchmarks show
  useful crossover points
- keep no-std/default builds free of CUDA dependencies
