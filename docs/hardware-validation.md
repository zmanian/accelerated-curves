# Hardware Validation

Use this runbook for cloud hosts that have CUDA or AVX-512 IFMA hardware. The
default CI remains CPU-only; these checks are burn-in evidence, not a default
consensus accept path.

## Common Setup

```sh
git clone --branch codex/accelerated-curves https://github.com/zmanian/accelerated-curves.git
cd accelerated-curves
mkdir -p repos
git clone --branch codex/pasta-msm-fallible https://github.com/zmanian/pasta-msm.git repos/pasta-msm
git clone --branch codex/accel-msm https://github.com/zmanian/ragu.git repos/ragu
git clone --branch codex/pasta-accel-dispatch https://github.com/zmanian/halo2.git repos/halo2
git clone --branch codex/halo2-accel-verify https://github.com/zmanian/zebra.git repos/zebra
rustup component add rustfmt clippy
```

Capture host evidence with the test logs:

```sh
rustc --version
cargo --version
uname -a
```

## CUDA Host

Requirements:

- Linux x86_64
- NVIDIA driver and visible GPU
- CUDA toolkit compatible with the driver
- `nvcc` available if the CUDA adapter build requires it

Capture:

```sh
nvidia-smi
nvcc --version
```

Run:

```sh
cargo test -p zcash-pasta-accel --features cuda -- --test-threads=1
cargo bench -p zcash-pasta-accel --features cuda --bench msm --no-run

cd repos/ragu
ZCASH_ACCEL=cuda ZCASH_ACCEL_MIN_MSM=1 \
  cargo test -p ragu_pcd --features accel-msm \
  seed_with_accel_config_falls_back_and_verifies -- --test-threads=1
cd ../..

cd repos/halo2
ZCASH_ACCEL=cuda ZCASH_ACCEL_MIN_MSM=1 \
  cargo test -p halo2_proofs --features pasta-accel -- --test-threads=1
cd ../..

cd repos/zebra
ZCASH_ACCEL=cuda ZCASH_ACCEL_MIN_MSM=1 \
  cargo test -p zebra-consensus --features halo2-accel-verify \
  block_with_garbage_orchard_proofs_is_rejected -- --test-threads=1
cd ../..
```

Expected result:

- CUDA availability should be reported by `pasta-msm`.
- `zcash-pasta-accel` should either return correct CUDA MSM results or clear
  Rust errors.
- Ragu, Halo2, and Zebra tests must still pass with CPU fallback or crosscheck
  acceptance semantics.

## AVX-512 IFMA Host

Requirements:

- Linux x86_64
- CPU with `avx512ifma`

Capture:

```sh
lscpu | grep -i avx512
```

Run:

```sh
cargo test -p zcash-pasta-accel --features avx512 -- --test-threads=1
cargo test -p zcash-pasta-accel --features avx512 \
  avx512_runtime_detection_does_not_claim_stub_backend_availability -- --test-threads=1
```

Expected result:

- `avx512_runtime_detected()` may report the CPU feature depending on the host.
- `Backend::Avx512` must still return `UnsupportedBackend` until a real
  AVX-512 MSM implementation is wired in.
- No downstream crate should accept AVX-512 as a production verifier backend
  from runtime detection alone.

## Evidence To Save

For each host, save:

- command output and exit status
- GPU or CPU capability output
- root commit, plus the Ragu, Halo2, Zebra, and pasta-msm branch SHAs
- whether any fallback counters incremented
- whether any crosscheck mismatch counters incremented

Do not run Sandblasting-style load against public nodes. Replay and stress
testing should stay local or offline.
