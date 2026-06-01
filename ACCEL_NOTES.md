# Pasta Acceleration Notes

Date: 2026-05-31

This file captures the baseline repository state and first integration points
for the Pasta acceleration plan. It is intentionally limited to inspection and
baseline scaffolding; consensus behavior is unchanged.

## Repository Snapshot

| Repo | Local path | Origin | Head |
| --- | --- | --- | --- |
| Zebra | `repos/zebra` | `https://github.com/ZcashFoundation/zebra.git` | `76c440e67` (`main`, `v4.5.1`) |
| Orchard | `repos/orchard` | `https://github.com/zcash/orchard.git` | `c6c93d5` (`main`) |
| Halo2 | `repos/halo2` | `https://github.com/zcash/halo2.git` | `32a87582` (`main`) |
| pasta_curves | `repos/pasta_curves` | `https://github.com/zcash/pasta_curves.git` | `fe08536` (`main`) |
| Ragu | `repos/ragu` | `https://github.com/tachyon-zcash/ragu.git` | `aa999eb2` (`main`) |
| Tachyon | `repos/tachyon` | `https://github.com/tachyon-zcash/tachyon.git` | `7a099b0` (`main`) |
| pasta-msm | `repos/pasta-msm` | `https://github.com/supranational/pasta-msm.git` | `861357b` (`main`) |
| sppark | `repos/sppark` | `https://github.com/supranational/sppark.git` | `eff682c` (`main`) |

## Clone Integration Status

The clone-side MVP is split between edited integration branches and
inspection-only baselines:

- `repos/pasta-msm` is edited to expose panic-free `try_pallas` and
  `try_vesta` APIs plus a CUDA runtime availability helper for the root
  adapter.
- `repos/ragu` is edited for an `accel-msm` arithmetic dispatcher, explicit
  PCD acceleration config plumbing, commitment call-site routing, fallback
  stats, and a PCD acceleration benchmark.
- `repos/halo2` is edited for a feature-gated `pasta-accel`
  `best_multiexp` dispatcher and facade dispatch stats while leaving
  `best_fft` CPU-only.
- `repos/zebra` is edited for safe Halo2 acceleration config parsing,
  candidate metrics, CPU-mode suppression, accelerated MSM/fallback metrics,
  and an offline replay benchmark skeleton.
- `repos/orchard`, `repos/pasta_curves`, `repos/tachyon`, and `repos/sppark`
  were cloned and inspected as dependency/reference baselines, but do not need
  local edits for the current MVP slice.

Fresh clone checks from the current pass:

- `git diff --check` in every cloned repo.
- `cargo test -- --test-threads=1` in `repos/pasta-msm`.
- `cargo test -p ragu_arithmetic --features accel-msm -- --test-threads=1`
  in `repos/ragu`.
- `cargo test -p halo2_proofs --features pasta-accel test_pasta_accel -- --test-threads=1`
  in `repos/halo2`.
- `cargo test -p zebra-consensus --features halo2-accel-verify config::tests::halo2_accel -- --test-threads=1`
  in `repos/zebra`.

## Dependency Graph

### Zebra and Orchard

Zebra currently consumes Orchard and Halo2 through crates.io:

- `repos/zebra/Cargo.lock`
  - `halo2_proofs = 0.3.2`
  - `orchard = 0.13.1`
  - `pasta_curves = 0.5.1`
- `repos/orchard/Cargo.lock`
  - `halo2_proofs = 0.3.2`
  - `pasta_curves = 0.5.1`

The Zebra consensus crate depends on `halo2_proofs` as package alias `halo2`:

- `repos/zebra/zebra-consensus/Cargo.toml:41`
  - `halo2 = { package = "halo2_proofs", version = "0.3" }`

### Ragu

Ragu is on newer pre-release field/group APIs and patches `pasta_curves`:

- `repos/ragu/Cargo.toml:70-72`
  - `ff = 0.14.0-pre.1`
  - `group = 0.14.0-pre.1`
  - `pasta_curves = 0.5.1`, feature `deferred`
- `repos/ragu/Cargo.toml:87-92`
  - `[patch.crates-io] pasta_curves` points to
    `https://github.com/ebfull/pasta_curves`
  - revision `4b2c768ec4b14b2c7fa0f6e527e453cde97805aa`

This means the first Ragu integration should be source-compatible with the
patched Ragu curve traits, not just crates.io `pasta_curves = 0.5.1`.

### Candidate CUDA Sources

- `repos/pasta-msm/Cargo.toml`
  - `pasta-msm = 0.1.5`
  - depends on `sppark ~0.1.14`
  - depends on `pasta_curves >=0.3.1, <=0.5`, feature `repr-c`
  - has `cuda-mobile`; CUDA itself is auto-detected in `build.rs`
- `repos/pasta-msm/src/lib.rs`
  - exposes `pallas(points, scalars)` and `vesta(points, scalars)`
  - now also exposes fallible `try_pallas(points, scalars)` and
    `try_vesta(points, scalars)`
  - fallible APIs return `Error::LengthMismatch`, return identity for empty
    MSMs, and map CUDA errors to `Error::Cuda(String)`
  - the legacy `pallas` and `vesta` APIs remain panic-style compatibility
    wrappers
  - falls back to CPU when CUDA is not selected
- `repos/sppark/README.md`
  - describes CUDA/C++ templates for MSM and NTT
  - explicitly includes Pasta curves as an MSM target

The production acceleration facade should not expose the current `pasta-msm`
panic behavior directly to Zebra, Orchard, Halo2, or Ragu.

## Halo2 Call Sites

Primary hook file:

- `repos/halo2/halo2_proofs/src/arithmetic.rs`
  - `best_multiexp` at line 143
  - `best_fft` at line 192

Known `best_multiexp` call sites:

- `repos/halo2/halo2_proofs/src/poly/commitment/msm.rs:169`
- `repos/halo2/halo2_proofs/src/poly/commitment/prover.rs:107-114`
- `repos/halo2/halo2_proofs/src/poly/commitment.rs:129`
- `repos/halo2/halo2_proofs/src/poly/commitment.rs:149`
- `repos/halo2/halo2_proofs/src/poly/commitment/verifier.rs:59`
- `repos/halo2/halo2_proofs/benches/msm.rs:20`

Known `best_fft` call sites:

- `repos/halo2/halo2_proofs/src/poly/commitment.rs:82`
- `repos/halo2/halo2_proofs/src/poly/domain.rs:249`
- `repos/halo2/halo2_proofs/src/poly/domain.rs:376`
- `repos/halo2/halo2_proofs/benches/fft.rs:19`

## Orchard and Zebra Verifier Path

Orchard batch validation:

- `repos/orchard/src/bundle/batch.rs`
  - `BatchValidator` stores `plonk::BatchVerifier<vesta::Affine>`
  - `add_bundle` queues bundle proofs and RedPallas signatures
  - `validate` first verifies RedPallas signatures, then calls
    `self.proofs.finalize(&vk.params, &vk.vk)`

Zebra async wrapper:

- `repos/zebra/zebra-consensus/src/primitives/halo2.rs`
  - imports `orchard::{bundle::BatchValidator, circuit::VerifyingKey}`
  - `Item::verify_single` builds a one-item `BatchValidator`
  - `Verifier::verify` calls `batch.validate(vk, thread_rng())`
  - `Verifier::flush_spawning` offloads `batch.validate` through `spawn_fifo`
  - existing metric: `zebra.consensus.batch.duration_seconds` with
    `verifier = "halo2"`

Zebra already has a baseline Halo2 Criterion bench:

- `repos/zebra/zebra-consensus/benches/halo2.rs`
  - extracts Orchard items from local mainnet test vectors
  - benchmarks `Item::verify_single`
  - cannot currently exercise true cross-bundle batching because `Item` fields
    are private to `zebra_consensus`

## Ragu Prover Hook Points

Primary MSM hook:

- `repos/ragu/crates/ragu_arithmetic/src/util.rs:186`
  - `pub fn mul<C, A, B>(coeffs, bases) -> C::Curve`
  - caller contract says coeff and base iterators must have the same length
  - implementation already uses windowed bucket MSM and `maybe-rayon`

Primary FFT hooks:

- `repos/ragu/crates/ragu_arithmetic/src/domain.rs`
  - `Domain::ring_fft`
  - `Domain::ring_ifft`
  - `Domain::fft`
  - `Domain::ifft`
- `repos/ragu/crates/ragu_arithmetic/src/util.rs`
  - `poly_mul`
  - `decomp_product_poly`
  - `poly_with_roots`

Ragu commitment hooks:

- `repos/ragu/crates/ragu_circuits/src/polynomials/sparse/mod.rs:466`
  - sparse polynomial `commit` calls `ragu_arithmetic::mul`
- `repos/ragu/crates/ragu_arithmetic/src/lib.rs:174`
  - `FixedGenerators::short_commit` is a tiny fixed-size commitment and should
    not be accelerated initially
- `repos/ragu/crates/ragu_pcd/src/proof/builder.rs`
  - several `commit_to_affine` call sites feed PCD proof construction
- `repos/ragu/crates/ragu_pcd/src/fuse/_06_ab.rs:114`
  - direct `ragu_arithmetic::mul` call in fuse flow

Existing Ragu baselines:

- `repos/ragu/crates/ragu_arithmetic/benches/criterion/msm.rs`
  - MSM sizes: `64, 256, 1024, 4096, 8192`
- `repos/ragu/crates/ragu_arithmetic/benches/criterion/fft.rs`
  - FFT/IFFT domain logs: `10, 14, 18`

## Local Deliverable 1 Scaffold

The root workspace now contains a standalone CPU-only facade crate:

- `crates/zcash-pasta-accel`

Implemented API:

- `Backend::{Cpu, Cuda, Avx512, Auto}`
- `DispatchConfig`
- `AccelError`
- `accelerated_backend_self_test`
- `avx512_runtime_detected`
- `backend_available`
- `backend_self_test`
- `dispatch_config`
- `min_msm_size`
- `msm_pallas`
- `msm_vesta`
- `with_dispatch_config`
- `record_msm_candidate`
- `record_msm_success`
- `record_msm_fallback`
- `reset_dispatch_stats`
- `take_dispatch_stats`

Current behavior:

- `Backend::Cpu` and `Backend::Auto` use pure Rust CPU MSM.
- `ZCASH_ACCEL` is parsed as `off|cpu|cuda|avx512|auto`.
- `ZCASH_ACCEL_MIN_MSM` is parsed as an MSM offload threshold, defaulting to
  `4096`.
- Scoped thread-local `DispatchConfig` overrides can set backend and threshold
  for verifier/prover integrations without mutating process-wide environment.
- Thread-local `DispatchStats` tracks candidate MSM calls, total candidate
  points, accelerated successes, and CPU fallbacks for integration metrics.
- `backend_self_test` runs deterministic Pallas and Vesta MSM comparisons
  against the CPU path; `accelerated_backend_self_test` only passes for non-CPU
  backends that are available and coherent.
- Unknown `ZCASH_ACCEL` values fall back to CPU.
- `Backend::Cuda` returns `AccelError::UnsupportedBackend` unless the CUDA
  feature is compiled; when compiled, it remains unavailable unless the cloned
  `pasta-msm` adapter reports real CUDA runtime availability.
- `avx512_runtime_detected` reports the CPU's `avx512ifma` capability for
  diagnostics, but `Backend::Avx512` remains unavailable and returns
  `AccelError::UnsupportedBackend` until a real AVX-512 MSM is implemented.
- `cuda` has an explicit build gate stub; until a CUDA adapter is implemented,
  `Backend::Cuda` depends on the optional cloned `pasta-msm` adapter but stays
  unavailable unless `pasta-msm` reports real CUDA runtime availability.
  CPU-only `pasta-msm` C++ fallback is not treated as CUDA availability.
- Length mismatches return `AccelError::LengthMismatch`.
- No unsafe code is used in the crate, and the crate denies unsafe operations
  inside unsafe functions for future FFI work.

## Local Deliverable 2 FFI Boundary

The facade crate now includes a CUDA-facing boundary module:

- `crates/zcash-pasta-accel/src/ffi.rs`
  - defines `#[repr(C)]` field, affine point, and projective point layouts
  - encodes Pasta field elements as four canonical little-endian `u64` limbs
  - uses an explicit `u8` point-at-infinity flag instead of an FFI `bool`
  - converts Pallas and Vesta affine points through checked coordinates
  - converts Pallas and Vesta projective points through checked Jacobian
    coordinates
  - rejects noncanonical field limbs and invalid curve coordinates with
    `AccelError::InvalidPoint`

This is a layout and validation skeleton plus an availability-gated adapter
path. The CUDA backend remains unavailable on hosts where `pasta-msm` was not
compiled with CUDA support or where CUDA runtime initialization is unavailable.

The cloned `pasta-msm` crate has been prepared for that adapter by adding
panic-free `try_pallas` and `try_vesta` APIs. Root `zcash-pasta-accel` does not
count the crate's CPU C++ fallback as CUDA availability; explicit
`Backend::Cuda` returns `AccelError::BackendUnavailable` in that case.

Current verification:

- `cargo test -p zcash-pasta-accel`
- `cargo test -p zcash-pasta-accel --features cuda`
- `cargo test -p zcash-pasta-accel --features avx512`
- `cargo test -p zcash-pasta-accel --features avx512 avx512_runtime_detection_does_not_claim_stub_backend_availability -- --test-threads=1`
- `cargo clippy -p zcash-pasta-accel --features cuda --all-targets -- -D warnings`
- `cargo bench -p zcash-pasta-accel --features cuda --bench msm --no-run`
- `cargo test --workspace`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo bench -p zcash-pasta-accel --bench msm --no-run`
- `cargo check --manifest-path crates/zcash-pasta-accel/fuzz/Cargo.toml --bin msm_inputs`
- `cargo +nightly fuzz check msm_inputs` in `crates/zcash-pasta-accel`
- `cargo test` in `repos/pasta-msm`
- `cargo clippy --all-targets -- -D warnings` in `repos/pasta-msm`
- Test coverage includes empty MSMs, one-point MSMs, zero scalars, identity
  points, repeated bases, high-value scalars, length mismatch, unsupported
  backends, backend self-test gating, larger seeded Pallas/Vesta cases, and
  proptest-generated Pallas/Vesta MSM equivalence cases.
- FFI test coverage includes C-layout sanity checks, little-endian field limb
  round trips, noncanonical field rejection, Pallas/Vesta affine round trips,
  invalid affine-coordinate rejection, identity flags, and Pallas/Vesta
  projective round trips.
- Fuzz coverage now starts with `crates/zcash-pasta-accel/fuzz`, whose
  `msm_inputs` target mutates Pallas/Vesta MSM lengths, signs, identity bases,
  backend selectors, and CPU-resolved Auto dispatch.
- `pasta-msm` test coverage now includes fallible length-mismatch handling and
  empty Pallas/Vesta MSM identity behavior, plus a safe CUDA runtime
  availability helper.

## Documentation

Initial documentation now lives under `docs/`:

- `docs/acceleration.md`
  - feature flags, env vars, verification commands, and current limitations
- `docs/acceleration-security.md`
  - consensus-safety rules and preconditions for experimental verifier accept
    mode
- `docs/zebra-halo2-accel.md`
  - Zebra feature/config/metric/replay benchmark status
- `docs/ragu-prover-accel.md`
  - Ragu MSM hook, verification commands, benchmark gaps, and remaining work
- `docs/hardware-validation.md`
  - cloud CUDA and AVX-512 validation commands, host evidence to capture, and
    expected fallback or stub behavior

## CI Matrix

The root repository now has a CPU-only default workflow:

- `.github/workflows/ci.yml`
  - clones `zmanian/pasta-msm` branch `codex/pasta-msm-fallible` before
    facade-dependent jobs so Cargo can resolve the optional local
    `pasta-msm` path dependency
  - runs `cargo fmt --all --check`
  - runs `cargo test --workspace`
  - runs `cargo test --workspace --no-default-features`
  - runs `cargo test -p zcash-pasta-accel`
  - runs `cargo clippy --workspace --all-targets -- -D warnings`
  - clones `zmanian/ragu` branch `codex/accel-msm` and runs
    `cargo test -p ragu_arithmetic --features accel-msm`
  - also runs focused Ragu PCD acceleration seed-path smoke tests:
    `cargo test -p ragu_pcd --features accel-msm seed_with_ -- --test-threads=1`
  - clones `zmanian/halo2` branch `codex/pasta-accel-dispatch` and runs
    `cargo test -p halo2_proofs --features pasta-accel`
  - clones `zmanian/halo2` plus `zmanian/zebra` branch
    `codex/halo2-accel-verify` and runs
    `cargo test -p zebra-consensus --features halo2-accel-verify`
  - keeps CUDA checks behind a manual `workflow_dispatch` `run_gpu` input:
    `cargo test -p zcash-pasta-accel --features cuda` and
    `cargo bench -p zcash-pasta-accel --features cuda --bench msm --no-run`
- `.github/workflows/hardware-validation.yml`
  - manual-only cloud-runner workflow with configurable `runs-on` labels
  - captures host, compiler, GPU/CPU feature, and branch SHA evidence
  - can run CUDA facade, Ragu, Halo2, and Zebra validation checks
  - can run AVX-512 runtime-detection and fallback-gating checks

## Ragu MSM Integration Status

Ragu now has a feature-gated MSM dispatcher on branch `codex/accel-msm`:

- `repos/ragu/crates/ragu_arithmetic/Cargo.toml`
  - adds `accel-msm = ["std", "dep:zcash-pasta-accel"]`
  - adds `accel-fft = ["std", "dep:zcash-pasta-accel"]` as a future hook
  - adds optional local path dependency on `zcash-pasta-accel`
- `repos/ragu/crates/ragu_arithmetic/benches/criterion/msm.rs`
  - prints `AccelMsmStats` when built with `accel-msm`, giving benchmark runs
    candidate, facade-result, fallback, and total candidate point counts
- `repos/ragu/crates/ragu_pcd/benches/criterion/pcd_accel.rs`
  - adds a Criterion benchmark target for end-to-end PCD operations
  - covers `seed`, `fuse`, `rerandomize`, and `verify`
  - compares CPU APIs with explicit `ProverAccelConfig` APIs under
    `accel-msm`
  - includes a forced CUDA fallback path to measure fallback overhead in
    CPU-only builds
  - prints `AccelMsmStats` after the group
- `repos/ragu/crates/ragu_arithmetic/src/util.rs`
  - keeps the existing windowed bucket implementation as `cpu_mul`
  - routes `mul` through `mul_with_accel_config` only under `accel-msm`
  - collects contiguous scalar/base vectors only when `accel-msm` is enabled
  - uses `ZCASH_ACCEL_MIN_MSM`, defaulting to `4096`, as the offload threshold
  - calls `zcash_pasta_accel::try_msm` with `Backend::Auto`
  - falls back to `cpu_mul` on unsupported curves, backend errors, or any
    accelerator miss
  - re-exports `zcash_pasta_accel::Backend` as `AccelBackend` for callers that
    need explicit backend selection without depending directly on the facade
  - adds explicit `AccelMsmConfig` and `mul_with_accel_config` for prover-side
    callers and tests that should not mutate process environment
  - records `AccelMsmStats` counters:
    - `candidates`
    - `facade_results`
    - `fallbacks`
    - `total_candidate_points`
- `repos/ragu/crates/ragu_circuits/Cargo.toml`
  - adds `accel-msm = ["std", "ragu_arithmetic/accel-msm"]`
- `repos/ragu/crates/ragu_circuits/src/polynomials/sparse/mod.rs`
  - adds `commit_with_accel_config` and
    `commit_to_affine_with_accel_config` under `accel-msm`
  - keeps the default commitment path unchanged, while giving prover-side tests
    and future config plumbing an explicit dispatcher entry point
- `repos/ragu/crates/ragu_circuits/src/polynomials/sparse/tests.rs`
  - verifies that forcing the unsupported CUDA backend through sparse
    polynomial commitment falls back to CPU and records one MSM candidate
- `repos/ragu/crates/ragu_pcd/Cargo.toml`
  - adds `accel-msm = ["std", "ragu_arithmetic/accel-msm", "ragu_circuits/accel-msm"]`
- `repos/ragu/crates/ragu_pcd/src/lib.rs`
  - adds feature-gated `ProverAccelConfig`
  - adds explicit `seed_with_accel_config` and
    `rerandomize_with_accel_config` APIs
- `repos/ragu/crates/ragu_pcd/src/fuse/mod.rs`
  - adds explicit `fuse_with_accel_config`
- `repos/ragu/crates/ragu_pcd/src/proof/builder.rs`
  - stores optional prover acceleration config on `ProofBuilder`
  - routes lazy native/nested commitment caches through explicit sparse
    commitment helpers when config is present
- `repos/ragu/crates/ragu_pcd/src/proof/mod.rs`
  - routes trivial proof construction through the same explicit config for
    `seed_with_accel_config`
- `repos/ragu/crates/ragu_pcd/src/fuse/*.rs`
  - routes direct bridge/native commitment construction through `ProofBuilder`
    helpers so the explicit config reaches real proof construction
- `repos/ragu/crates/ragu_pcd/tests/rerandomization.rs`
  - keeps the normal forced-fallback seed proof regression
  - compiles ignored/manual forced-fallback regressions for explicit-config
    `fuse` and `rerandomize` proof paths, preserving broader coverage without
    slowing the default focused loop

Ragu verification run so far:

- `cargo test -p ragu_arithmetic`
- `cargo test -p ragu_arithmetic --features accel-msm`
- `cargo test -p ragu_arithmetic --features accel-msm test_accel_msm_records_forced_backend_fallback -- --test-threads=1`
- `cargo test -p ragu_circuits --features accel-msm commit_with_accel_config_records_forced_backend_fallback -- --test-threads=1`
- `cargo test -p ragu_circuits --features accel-msm commit_matches_dense -- --test-threads=1`
- `cargo test -p ragu_pcd --features accel-msm with_accel_config_falls_back -- --test-threads=1`
- `cargo test -p ragu_pcd --features accel-msm seed_with_accel_config_falls_back_and_verifies -- --test-threads=1`
- `cargo test -p ragu_pcd --features accel-msm seed_with_cuda_accel_config_dispatches_or_falls_back_and_verifies -- --test-threads=1`
- `cargo test -p ragu_pcd --features accel-msm seed_with_ -- --test-threads=1`
- `cargo check -p ragu_arithmetic --no-default-features --features alloc`
- `cargo check -p ragu_pcd --no-default-features --features alloc`
- `cargo clippy -p ragu_arithmetic --features accel-msm --all-targets -- -D warnings`
- `cargo clippy -p ragu_pcd --features accel-msm --all-targets -- -D warnings`
- `cargo bench -p ragu_arithmetic --features accel-msm --bench msm_criterion --no-run`
- `cargo bench -p ragu_arithmetic --features accel-msm --bench fft_criterion --no-run`
- `cargo bench -p ragu_pcd --bench pcd_accel_criterion --no-run`
- `cargo bench -p ragu_pcd --features accel-msm --bench pcd_accel_criterion --no-run`
- `cargo clippy -p ragu_pcd --features accel-msm --all-targets -- -D warnings`

## Halo2 MSM Integration Status

Halo2 now has a feature-gated `best_multiexp` dispatcher on branch
`codex/pasta-accel-dispatch`:

- `repos/halo2/halo2_proofs/Cargo.toml`
  - adds optional local path dependency on `zcash-pasta-accel`
  - adds `pasta-accel = ["zcash-pasta-accel"]`
- `repos/halo2/halo2_proofs/src/arithmetic.rs`
  - keeps the existing implementation as `cpu_best_multiexp`
  - uses the existing CPU path when `pasta-accel` is disabled
  - under `pasta-accel`, tries `zcash_pasta_accel::try_msm` once the MSM size
    reaches `ZCASH_ACCEL_MIN_MSM`, default `4096`
  - falls back to CPU on unsupported curves, backend errors, or forced backend
    failure such as `ZCASH_ACCEL=cuda` in a CPU-only build
  - records facade dispatch stats for MSM candidate points, accelerated
    successes, and fallback paths
  - keeps FFT behavior unchanged by routing `best_fft` through explicit
    `cpu_best_fft`, giving future FFT acceleration work a CPU comparator

Halo2 verification run so far:

- `cargo test -p halo2_proofs`
- `cargo test -p halo2_proofs --features pasta-accel`
- `cargo test -p halo2_proofs --features pasta-accel test_pasta_accel_multiexp_matches_cpu`
- `cargo test -p halo2_proofs --features pasta-accel test_pasta_accel_best_fft_matches_cpu`
- `cargo test -p halo2_proofs --features pasta-accel test_pasta_accel_records_dispatch_stats -- --test-threads=1`
- `cargo test -p halo2_proofs --features pasta-accel test_pasta_accel -- --test-threads=1`
- `cargo clippy -p halo2_proofs --all-targets -- -D warnings`
- `cargo clippy -p halo2_proofs --features pasta-accel --all-targets -- -D warnings`
- `cargo bench -p halo2_proofs --features pasta-accel --bench msm --no-run`
- `cargo bench -p halo2_proofs --features pasta-accel --bench fft --no-run`

## Zebra Halo2 Accel Feature Bridge

Zebra now has a feature-gated bridge to the cloned Halo2 accelerator branch on
branch `codex/halo2-accel-verify`:

- `repos/zebra/Cargo.toml`
  - patches crates.io `halo2_proofs` to `../halo2/halo2_proofs`
- `repos/zebra/zebra-consensus/Cargo.toml`
  - adds `halo2-accel-verify = ["halo2/pasta-accel"]`
- `repos/zebra/zebrad/Cargo.toml`
  - adds `halo2-accel-verify = ["zebra-consensus/halo2-accel-verify"]`
- `repos/zebra/zebra-consensus/src/config.rs`
  - adds `consensus.halo2_accel` with safe defaults:
    - `enabled = false`
    - `backend = "auto"`
    - `mode = "cpu"`
    - `min_batch_actions = 64`
    - `min_msm_size = 4096`
  - supports TOML values such as:
    - `backend = "avx512"`
    - `mode = "cpu"`
    - `mode = "crosscheck"`
    - `mode = "experimental-accept"`
  - suppresses acceleration candidates when `mode = "cpu"` even if
    `enabled = true`
  - exposes startup status showing whether the accel feature is compiled,
    whether the requested backend is available, whether backend self-tests
    passed, and whether experimental accept is actually ready
- `repos/zebra/zebra-consensus/src/router.rs`
  - passes `consensus.halo2_accel` into the transaction verifier at startup
  - reports operator-facing Halo2 acceleration startup status before verifier
    services are constructed
- `repos/zebra/zebra-consensus/src/transaction.rs`
  - stores the Halo2 accel config on the transaction verifier
  - records Orchard bundle candidate metrics before queuing into the existing
    Halo2 verifier:
    - `zebra.consensus.halo2_accel.candidate_total`
    - labels: `backend`, `candidate`, `compiled`, `enabled`, `mode`
- `repos/zebra/zebra-consensus/src/primitives/halo2.rs`
  - stores `Halo2AccelConfig` on queued Orchard/Halo2 items
  - accumulates action counts and candidate item counts for each batch flush
  - installs scoped `zcash-pasta-accel::DispatchConfig` around feature-enabled
    batch verification instead of mutating process environment
  - keeps a CPU-only copy of each batch for crosscheck mode
  - in `crosscheck` mode, records accelerated and CPU results and accepts
    according to the CPU result
  - in `experimental-accept` mode, falls back to CPU-protected crosscheck
    unless Zebra is built with `experimental-verifier-accept` and the requested
    non-CPU backend passes `accelerated_backend_self_test`
  - records batch-service metrics:
    - `zebra.consensus.halo2.accel.backend`
    - `zebra.consensus.halo2.accel.mode`
    - `zebra.consensus.halo2.accel.batch_actions`
    - `zebra.consensus.halo2.accel.duration_seconds`
    - `zebra.consensus.halo2.accel.crosscheck_mismatches`
    - `zebra.consensus.halo2.accel.msm_points`
    - `zebra.consensus.halo2.accel.fallbacks`
    - labels: `backend`, `candidate`, `compiled`, `enabled`, `mode`, and
      `result` on duration or `mismatch` on crosscheck mismatch counts
    - MSM/fallback metrics are emitted only for batches Zebra classified as
      acceleration candidates
  - resets and drains facade dispatch stats around CPU crosscheck validation so
    CPU-only crosschecks do not contaminate accelerated-path metrics
  - exposes a `fuzz-impl` helper module for the standalone `cargo-fuzz`
    target; the production API remains encapsulated unless that feature is
    explicitly enabled
- `repos/zebra/zebra-consensus/fuzz`
  - adds a standalone `cargo-fuzz` package for consensus fuzz targets
  - `halo2_batch_items` mutates real Orchard/Halo2 items extracted from local
    Zebra block test vectors
  - mutates batch length, source item selection, backend, mode,
    `min_batch_actions`, and `min_msm_size`
  - checks that Zebra's batch summary agrees with real item action counts,
    candidate counts, CPU-mode suppression, crosscheck gating, and
    experimental-accept gating invariants
- `repos/zebra/zebra-consensus/src/transaction/tests.rs`
  - broadens the CVE-2026-34377 mempool-cache regression coverage with
    txid-preserving Orchard auth-data mutations for proof bytes, binding
    signatures, and spend authorization signatures

The config now reaches the runtime Orchard bundle queue point, Halo2 batch
service metrics, scoped Halo2 MSM dispatch, and accelerated-path MSM/fallback
metrics. Default Zebra consensus behavior remains unchanged, and crosscheck
mode remains CPU-accepting.

Zebra verification run so far:

- `cargo test -p zebra-consensus config::tests::halo2_accel -- --test-threads=1`
- `cargo test -p zebra-consensus --features halo2-accel-verify config::tests::halo2_accel -- --test-threads=1`
- `cargo test -p zebra-consensus --features halo2-accel-verify halo2_accel_startup_status_reports_failed_avx512_stub_self_test -- --test-threads=1`
- `cargo test -p zebra-consensus halo2_batch_accel_context_tracks_candidate_actions -- --test-threads=1`
- `cargo test -p zebra-consensus --features halo2-accel-verify halo2_batch_accel_context_tracks_candidate_actions -- --test-threads=1`
- `cargo test -p zebra-consensus --features halo2-accel-verify halo2_batch_accel_context -- --test-threads=1`
- `cargo test -p zebra-consensus halo2_batch_accel_records_backend_and_mode_selection_metrics -- --test-threads=1`
- `cargo test -p zebra-consensus --features halo2-accel-verify halo2_batch_accel_takes_facade_dispatch_stats -- --test-threads=1`
- `cargo test -p zebra-consensus --features halo2-accel-verify halo2_batch_accel -- --test-threads=1`
- `cargo test -p zebra-consensus --features experimental-verifier-accept halo2_batch_accel_experimental_accept -- --test-threads=1`
- `cargo test -p zebra-consensus block_with_garbage_orchard_proofs_is_rejected -- --test-threads=1`
- `cargo test -p zebra-consensus block_with_individual_orchard_auth_data_mutations_is_rejected -- --test-threads=1`
- `cargo test -p zebra-consensus --features halo2-accel-verify block_with_garbage_orchard_proofs_is_rejected -- --test-threads=1`
- `cargo test -p zebra-consensus --features experimental-verifier-accept block_with_garbage_orchard_proofs_is_rejected -- --test-threads=1`
- `cargo check -p zebra-consensus --features halo2-accel-verify`
- `cargo check -p zebrad --no-default-features --features halo2-accel-verify`
- `cargo check -p zebrad --no-default-features --features experimental-verifier-accept`
- `cargo clippy -p zebra-consensus --features halo2-accel-verify --all-targets -- -D warnings`
- `cargo clippy -p zebra-consensus --features experimental-verifier-accept --all-targets -- -D warnings`
- `cargo check --manifest-path zebra-consensus/fuzz/Cargo.toml --bin halo2_batch_items`
- `cargo +nightly fuzz check --fuzz-dir zebra-consensus/fuzz halo2_batch_items`
- `cargo +nightly fuzz check --fuzz-dir zebra-consensus/fuzz halo2_batch_items --features halo2-accel-verify`
- `cargo +nightly fuzz run --fuzz-dir zebra-consensus/fuzz halo2_batch_items -- -runs=1`
- `cargo clippy --manifest-path zebra-consensus/fuzz/Cargo.toml --bin halo2_batch_items --features halo2-accel-verify -- -D warnings`

## Zebra Offline Replay Benchmark

Zebra now has an offline Orchard/Halo2 replay benchmark skeleton:

- `repos/zebra/zebra-consensus/benches/common.rs`
  - exposes `extract_halo2_items_from_blocks()` for local NU5+ mainnet test
    vectors
- `repos/zebra/zebra-consensus/benches/halo2.rs`
  - reuses the shared extraction helper
- `repos/zebra/zebra-consensus/benches/halo2_sandblast_replay.rs`
  - replays local test-vector Orchard bundles only
  - compares unbatched single-bundle verification with the current async
    Halo2 batch service
  - adds a feature-gated `crosscheck_batch_service` series under
    `halo2-accel-verify`
  - reports throughput in Orchard actions
  - uses repeated local bundles to approximate backlog pressure without any
    public-network load generation

Replay benchmark verification run so far:

- `cargo bench -p zebra-consensus --bench halo2_sandblast_replay --no-run`
- `cargo bench -p zebra-consensus --features halo2-accel-verify --bench halo2_sandblast_replay --no-run`
- `cargo bench -p zebra-consensus --bench halo2 --no-run`

Known caveat:

- feature-enabled Zebra checks compile through the cloned Halo2 and root
  `zcash-pasta-accel` crates, and crosscheck mode is CPU-accepting. Production
  experimental accept now has regression coverage showing the garbage Orchard
  auth-data rejection path still rejects in CPU, crosscheck, experimental
  fallback, and experimental-accept builds, plus startup backend self-test
  reporting. It also covers individual txid-preserving Orchard auth-data
  mutations in CPU mode. It still needs a real invalid-proof corpus built from
  valid Orchard bundles and hardware burn-in evidence.
- the replay benchmark currently uses repeated local test-vector bundles, not
  historical Sandblasting-era block data or synthetic valid Orchard bundles.
