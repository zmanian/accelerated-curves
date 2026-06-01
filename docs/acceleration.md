# Pasta Acceleration

This workspace contains an optional acceleration stack for Pasta-curve MSM-heavy
workloads in Ragu, Halo2, and Zebra.

The default behavior is conservative:

- CPU remains the default backend.
- CUDA is not built or required by default.
- AVX-512 is only selected after runtime feature detection.
- Zebra verifier behavior is unchanged unless explicitly configured in future
  verifier modes.

## Crates And Repositories

- `crates/zcash-pasta-accel`
  - shared acceleration facade
  - currently implements CPU MSM for Pallas and Vesta
  - exposes CUDA and AVX-512 backend selectors as feature-gated/future paths
  - exposes AVX-512 runtime detection for diagnostics without claiming the
    stub is a usable backend
  - includes an optional `pasta-msm` CUDA adapter behind `cuda`
  - treats CUDA as available only when `pasta-msm` reports real CUDA runtime
    availability
  - exposes deterministic CPU-vs-backend self-tests for verifier/prover safety
    gates
  - exposes thread-local dispatch stats for candidate MSM points, accelerated
    successes, and CPU fallbacks
  - includes checked `#[repr(C)]` FFI field/point layouts for future CUDA
    adapters
- `repos/ragu`
  - `ragu_arithmetic` has an `accel-msm` feature that dispatches large Pasta
    MSMs through `zcash-pasta-accel`
- `repos/halo2`
  - `halo2_proofs` has a `pasta-accel` feature that dispatches large
    `best_multiexp` calls through `zcash-pasta-accel`
  - `pasta-accel` records facade dispatch stats around accelerated MSM
    candidates and fallback paths
  - `best_fft` is still CPU-only, with `cpu_best_fft` available as a future
    acceleration comparator
- `repos/zebra`
  - `zebra-consensus` and `zebrad` have a `halo2-accel-verify` feature that
    builds against the local accelerated Halo2 branch
  - crosscheck mode uses scoped acceleration dispatch and accepts according to
    the CPU verifier result
  - batch metrics include candidate action counts, duration, crosscheck
    mismatches, accelerated MSM point totals, and fallback counts
  - experimental accept is behind an additional explicit build feature and
    requires the configured non-CPU backend to pass the facade self-test before
    it can accept accelerated results

## Feature Flags

Root acceleration crate:

- `std`
- `cuda`
- `avx512`
- `ragu`
- `halo2`
- `experimental-verifier-accept`

Ragu:

- `accel-msm`
- `accel-fft` reserved for future FFT work

Halo2:

- `pasta-accel`

Zebra:

- `halo2-accel-verify`
- `experimental-verifier-accept`

## Environment Variables

- `ZCASH_ACCEL=off|cpu|cuda|avx512|auto`
  - defaults to CPU behavior when unset
  - unknown values fall back to CPU
- `ZCASH_ACCEL_MIN_MSM=<usize>`
  - default threshold is `4096`
  - used by Ragu and Halo2 MSM dispatch when no scoped dispatch config is set
- `ZCASH_ACCEL_VERIFY_MODE=cpu|crosscheck|experimental-accept`
  - documented for planned Zebra verifier modes
  - Zebra verifier modes are currently selected by TOML config rather than this
    process environment variable
- `ZCASH_ACCEL_LOG=0|1`
  - reserved for future backend diagnostics

Verifier/prover integrations can also use
`zcash_pasta_accel::with_dispatch_config` to install a thread-local backend and
threshold override without changing process-wide environment variables.

## Verification Commands

The root repository also has a default GitHub Actions matrix in
`.github/workflows/ci.yml`. It runs the non-GPU root checks below, then clones
the personal Ragu, Halo2, and Zebra integration branches under `repos/` so
their path dependencies resolve back to `crates/zcash-pasta-accel`. The Zebra
job also compile-checks and lints the Halo2 fuzz targets that exercise batch
item summaries and invalid Orchard auth-data mutations.

CUDA checks are manual-only through `workflow_dispatch` with `run_gpu=true`.
For cloud-host validation, use `.github/workflows/hardware-validation.yml` or
the commands in `docs/hardware-validation.md`.

Acceleration facade:

```sh
cargo test -p zcash-pasta-accel -- --test-threads=1
cargo test -p zcash-pasta-accel --features cuda -- --test-threads=1
cargo test -p zcash-pasta-accel --features avx512 -- --test-threads=1
cargo test -p zcash-pasta-accel --features avx512 avx512_runtime_detection_does_not_claim_stub_backend_availability -- --test-threads=1
cargo clippy -p zcash-pasta-accel --all-targets -- -D warnings
cargo clippy -p zcash-pasta-accel --features cuda --all-targets -- -D warnings
cargo bench -p zcash-pasta-accel --bench msm --no-run
cargo check --manifest-path crates/zcash-pasta-accel/fuzz/Cargo.toml --bin msm_inputs
(cd crates/zcash-pasta-accel && cargo +nightly fuzz run msm_inputs)
```

Ragu:

```sh
cargo test -p ragu_arithmetic
cargo test -p ragu_arithmetic --features accel-msm
cargo test -p ragu_pcd --features accel-msm seed_with_ -- --test-threads=1
cargo check -p ragu_arithmetic --no-default-features --features alloc
cargo bench -p ragu_arithmetic --features accel-msm --bench msm_criterion --no-run
cargo bench -p ragu_pcd --features accel-msm --bench pcd_accel_criterion --no-run
```

Halo2:

```sh
cargo test -p halo2_proofs --features pasta-accel test_pasta_accel -- --test-threads=1
cargo clippy -p halo2_proofs --features pasta-accel --all-targets -- -D warnings
cargo bench -p halo2_proofs --features pasta-accel --bench msm --no-run
cargo bench -p halo2_proofs --features pasta-accel --bench fft --no-run
```

Zebra:

```sh
cargo test -p zebra-consensus --features halo2-accel-verify config::tests::halo2_accel -- --test-threads=1
cargo test -p zebra-consensus --features halo2-accel-verify halo2_batch_accel_context -- --test-threads=1
cargo check -p zebrad --no-default-features --features halo2-accel-verify
cargo check -p zebrad --no-default-features --features experimental-verifier-accept
cargo clippy -p zebra-consensus --features halo2-accel-verify --all-targets -- -D warnings
cargo clippy -p zebra-consensus --features experimental-verifier-accept --all-targets -- -D warnings
cargo bench -p zebra-consensus --bench halo2_sandblast_replay --no-run
cargo check --manifest-path zebra-consensus/fuzz/Cargo.toml --bin halo2_batch_items
cargo +nightly fuzz check --fuzz-dir zebra-consensus/fuzz halo2_batch_items
cargo +nightly fuzz check --fuzz-dir zebra-consensus/fuzz halo2_batch_items --features halo2-accel-verify
cargo +nightly fuzz run --fuzz-dir zebra-consensus/fuzz halo2_batch_items -- -runs=1
cargo check --manifest-path zebra-consensus/fuzz/Cargo.toml --bin halo2_invalid_proofs
cargo +nightly fuzz check --fuzz-dir zebra-consensus/fuzz halo2_invalid_proofs --features halo2-accel-verify
cargo +nightly fuzz run --fuzz-dir zebra-consensus/fuzz halo2_invalid_proofs -- -runs=1
```

## FFI Boundary

`zcash-pasta-accel::ffi` defines the current GPU-facing data boundary:

- `FfiFieldElement`
- `FfiAffinePoint`
- `FfiProjectivePoint`

Field elements use four canonical little-endian `u64` limbs. Affine and
projective points carry an explicit `u8` infinity flag. Conversions currently
round-trip Pallas and Vesta affine/projective points and reject noncanonical
fields or invalid curve coordinates before any future foreign call.

## Current Limitations

- The CUDA MSM adapter is availability-gated; `--features cuda` compiles the
  optional `pasta-msm` path, but `Backend::Cuda` returns
  `BackendUnavailable` unless `pasta-msm` reports real CUDA runtime
  availability.
- The cloned `pasta-msm` crate has fallible `try_pallas`/`try_vesta` wrappers.
  The root facade maps length mismatches, CUDA errors, and unavailable runtime
  state into `AccelError` instead of exposing panics.
- AVX-512 currently exposes runtime detection only. `Backend::Avx512` remains
  unavailable and returns `UnsupportedBackend` until a real AVX-512 MSM is
  implemented, so it cannot satisfy accelerated verifier self-tests by
  returning the CPU result.
- Ragu acceleration is MSM-only.
- Zebra crosscheck mode is CPU-protected, and experimental accept is gated on
  facade backend self-tests. The garbage Orchard auth-data regression is
  covered across modes, individual txid-preserving auth-data mutations are
  covered in CPU mode, and startup logging reports backend availability and
  self-test readiness. The `halo2_batch_items` fuzz target covers batch-item
  selection and acceleration gating invariants over real local test-vector
  items. The `halo2_invalid_proofs` fuzz target covers proof, binding-signature,
  and spend-authorization mutations derived from valid local Orchard/Halo2
  items.
- The offline replay benchmark uses repeated local test-vector bundles, not a
  historical Sandblasting block corpus.
