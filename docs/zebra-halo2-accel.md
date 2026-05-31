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

Planned metrics still needed by the implementation plan:

- `zebra.consensus.halo2.accel.backend`
- `zebra.consensus.halo2.accel.mode`

## Offline Replay

Compile the offline replay benchmark:

```sh
cargo bench -p zebra-consensus --bench halo2_sandblast_replay --no-run
```

The current replay benchmark:

- uses local Zebra test vectors only
- repeats local Orchard bundles to approximate backlog pressure
- pins CPU mode
- compares unbatched verification with the existing async batch service
- reports throughput in Orchard actions

It does not generate or send network load.

## Remaining Work

- promote the backend self-test gate into explicit startup telemetry/operator
  reporting
- add backend and mode gauges once those runtime paths can observe backend
  decisions directly
- add invalid-proof rejection coverage for every mode
- replace repeated local bundles with historical or synthetic valid offline
  replay corpora when available
