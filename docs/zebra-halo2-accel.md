# Zebra Halo2 Acceleration

Zebra integration is intentionally opt-in and conservative. The current branch
builds Zebra against the local accelerated Halo2 branch, parses runtime config,
records candidate metrics before Orchard bundles enter the existing Halo2
verifier, and carries the config into the Halo2 batch service for batch-level
acceleration metrics and scoped dispatch.

Crosscheck mode is CPU-protected: Zebra runs the configured accelerated path and
a CPU-only verifier path, records mismatches, and accepts according to the CPU
result. Experimental accept mode remains gated behind an explicit build feature
and a deterministic non-CPU backend self-test; burn-in evidence is still needed
before use.

At router startup, Zebra reports the configured acceleration status for
operators: build support, backend availability, backend self-test result,
thresholds, and whether experimental accept is actually ready. If experimental
accept is configured but not ready, Zebra reports that it is keeping
CPU-protected crosscheck behavior.

## Feature Flag

Build Zebra with:

```sh
cargo check -p zebrad --no-default-features --features halo2-accel-verify
```

Experimental accept builds must additionally opt in:

```sh
cargo check -p zebrad --no-default-features --features experimental-verifier-accept
```

The feature flow is:

- `zebrad/halo2-accel-verify`
- `zebra-consensus/halo2-accel-verify`
- `halo2/pasta-accel`
- local `zcash-pasta-accel`

The experimental accept feature flow is:

- `zebrad/experimental-verifier-accept`
- `zebra-consensus/experimental-verifier-accept`
- `zebra-consensus/halo2-accel-verify`

The Zebra workspace patches `halo2_proofs` to `../halo2/halo2_proofs`.

## Config

Current config shape:

```toml
[consensus.halo2_accel]
enabled = false
backend = "auto"
mode = "cpu"
min_batch_actions = 64
min_msm_size = 4096
```

Supported backend strings:

- `auto`
- `cuda`
- `avx512`

Supported mode strings:

- `cpu`
- `crosscheck`
- `experimental-accept`

In `cpu` mode, Zebra keeps current CPU verifier behavior and suppresses
acceleration candidates even if `enabled = true`.

In `crosscheck` mode, candidate batches use scoped `zcash-pasta-accel`
dispatch config for the accelerated run, then rerun a CPU-only verifier copy and
accept according to the CPU result.

In `experimental-accept` mode without the `experimental-verifier-accept` build
feature, Zebra keeps the CPU-protected crosscheck behavior. With the feature
enabled, candidate batches can accept the accelerated result only if the
configured backend passes `zcash_pasta_accel::accelerated_backend_self_test`.
If the self-test fails or the backend is unavailable, Zebra keeps the
CPU-protected crosscheck behavior.

The config reaches `transaction::Verifier` from `router::init`, then the
Orchard queue point in `verify_orchard_bundle`, and finally the
`primitives::halo2::Item` queued into the batch verifier.

## Invalid Proof Coverage

Zebra's CVE-2026-34377 regression test for a block transaction with garbage
Orchard proof bytes now runs through the configurable verifier path. The test
matrix covers:

- default CPU behavior
- crosscheck mode with `halo2-accel-verify`
- experimental-accept config without the accept feature, which must fall back
  to CPU-protected crosscheck behavior
- experimental-accept config with the `experimental-verifier-accept` build
  feature

The same scenario also has CPU-mode coverage for individual txid-preserving
Orchard auth-data mutations: proof bytes, binding signature, and spend
authorization signature. The current coverage proves that these local
mempool-cache bypass shapes keep rejecting. It is not yet a valid-bundle-derived
invalid-proof corpus or hardware burn-in substitute.

## Metrics

Current transaction-level metric:

- `zebra.consensus.halo2_accel.candidate_total`

Labels:

- `backend`
- `candidate`
- `compiled`
- `enabled`
- `mode`

Current batch-service metrics:

- `zebra.consensus.halo2.accel.backend`
- `zebra.consensus.halo2.accel.mode`
- `zebra.consensus.halo2.accel.batch_actions`
- `zebra.consensus.halo2.accel.duration_seconds`
- `zebra.consensus.halo2.accel.crosscheck_mismatches`
- `zebra.consensus.halo2.accel.msm_points`
- `zebra.consensus.halo2.accel.fallbacks`

Labels:

- `backend`
- `candidate`
- `compiled`
- `enabled`
- `mode`
- `result` on duration only
- `mismatch` on crosscheck mismatch counter only
- `candidate` is not attached to `msm_points` or `fallbacks`; those metrics
  are only emitted for batches Zebra classified as acceleration candidates

## Offline Replay

Compile the offline replay benchmark:

```sh
cargo bench -p zebra-consensus --bench halo2_sandblast_replay --no-run
cargo bench -p zebra-consensus --features halo2-accel-verify --bench halo2_sandblast_replay --no-run
```

The current replay benchmark:

- uses local Zebra test vectors only
- repeats local Orchard bundles to approximate backlog pressure
- compares unbatched verification with the existing async batch service
- adds a feature-gated crosscheck batch-service series when built with
  `halo2-accel-verify`
- reports throughput in Orchard actions

It does not generate or send network load.

## Fuzzing

`zebra-consensus/fuzz/halo2_batch_items` is a standalone `cargo-fuzz` target
for the Halo2 batch-item acceleration gate.

The target extracts real Orchard/Halo2 items from Zebra's local block test
vectors, then mutates:

- batch length
- source item selection
- backend selection
- verifier mode
- `min_batch_actions`
- `min_msm_size`

It verifies that Zebra's fuzz-only batch summary matches the real queued item
action counts, candidate counts, CPU-mode suppression, crosscheck gating, and
experimental-accept gating invariants. It does not perform public-network load
generation.

Useful commands:

```sh
cargo check --manifest-path zebra-consensus/fuzz/Cargo.toml --bin halo2_batch_items
cargo +nightly fuzz check --fuzz-dir zebra-consensus/fuzz halo2_batch_items
cargo +nightly fuzz check --fuzz-dir zebra-consensus/fuzz halo2_batch_items --features halo2-accel-verify
cargo +nightly fuzz run --fuzz-dir zebra-consensus/fuzz halo2_batch_items -- -runs=1
cargo clippy --manifest-path zebra-consensus/fuzz/Cargo.toml --bin halo2_batch_items --features halo2-accel-verify -- -D warnings
```

## Remaining Work

- build a valid-bundle-derived invalid-proof corpus beyond the current local
  auth-data mutation regressions
- add an experimental-accept replay series once hardware burn-in evidence is
  available
- replace repeated local bundles with historical or synthetic valid offline
  replay corpora when available
