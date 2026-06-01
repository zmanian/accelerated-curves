# Acceleration Security Notes

Pasta acceleration touches consensus-sensitive verification code. The security
posture is CPU-first until accelerated backends have enough differential and
replay evidence.

## Hard Rules

1. Never skip Orchard/Halo2 proof verification.
2. Never accept a proof solely because an accelerator says so unless the build
   and runtime config both explicitly enable experimental accept mode.
3. Crosscheck mode must accept only if CPU verification accepts.
4. Any accelerator mismatch must emit metrics and fall back or reject according
   to the configured mode.
5. Any backend error must fall back to CPU for proving paths and CPU verifier
   behavior for consensus paths.
6. Default builds must not require CUDA or AVX-512 hardware.
7. Unsafe FFI must stay isolated inside the acceleration crate.
8. Do not log witness values, scalar buffers, raw proof internals, or private
   input buffers.
9. Do not run Sandblasting-style load against public nodes.

## Current Safety Boundaries

- `zcash-pasta-accel` has no unsafe code in the current CPU facade and denies
  unsafe operations inside unsafe functions for future FFI work.
- The current FFI boundary is Rust-only layout and validation code: canonical
  field parsing, checked curve-coordinate parsing, and explicit infinity flags.
- The cloned `pasta-msm` crate exposes fallible wrappers so future integration
  can map length mismatches and CUDA errors to Rust errors instead of panics.
- `Backend::Cuda` returns `UnsupportedBackend` without the `cuda` feature and
  `BackendUnavailable` when `cuda` is compiled but real CUDA runtime
  availability is absent.
- `Backend::Avx512` exposes runtime CPU feature detection for diagnostics, but
  the backend remains unavailable and returns `UnsupportedBackend` until a real
  AVX-512 MSM implementation exists.
- `backend_self_test` compares deterministic Pallas and Vesta MSMs against the
  CPU path; `accelerated_backend_self_test` only passes for non-CPU backends.
- Zebra reports startup status for configured Halo2 acceleration, including
  build support, backend availability, backend self-test result, thresholds,
  and experimental-accept readiness.
- Ragu falls back to its existing CPU `mul` implementation when the accelerator
  is unavailable, unsupported, or errors.
- Halo2 falls back to `cpu_best_multiexp` when `zcash-pasta-accel` cannot
  return a Pasta MSM result.
- Zebra crosscheck mode uses scoped acceleration dispatch for one verifier run,
  reruns a CPU-only verifier copy, records mismatch metrics, and accepts
  according to the CPU result.
- Zebra experimental accept mode requires an explicit build feature and a
  passing non-CPU backend self-test before it can accept the accelerated result
  directly.

## Before Experimental Verifier Accept Mode

Experimental accept mode needs all of the following before it can change
consensus behavior:

- explicit `experimental-verifier-accept` build feature
- explicit runtime config
- backend availability checks
- CPU fallback on backend errors
- crosscheck burn-in with zero mismatches
- broader invalid-proof rejection tests beyond the current garbage Orchard
  proof regression
- offline replay evidence with mismatch and fallback counters

Until then, Zebra acceleration work should stay in CPU or crosscheck mode.
