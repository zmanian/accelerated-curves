//! Optional acceleration facade for Pasta-curve MSM-heavy workloads.
//!
//! The default implementation is deliberately conservative: it exposes the
//! backend-selection API that downstream crates can integrate against, but only
//! the pure Rust CPU path is available unless future feature-gated backends are
//! added.

#![deny(unsafe_op_in_unsafe_fn)]

use core::any::Any;
use std::cell::Cell;

use pasta_curves::{
    arithmetic::CurveAffine,
    group::{ff::Field, Curve, Group},
    pallas, vesta,
};

pub mod ffi;

/// Acceleration backend requested by the caller.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Backend {
    /// Always use the pure Rust CPU implementation.
    Cpu,
    /// Use CUDA when compiled and available.
    Cuda,
    /// Use an AVX-512 implementation when compiled and available.
    Avx512,
    /// Pick an available backend automatically.
    Auto,
}

/// Default minimum MSM size before downstream dispatchers consider acceleration.
pub const DEFAULT_MIN_MSM: usize = 4096;

/// Effective dispatch settings for downstream integration points.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DispatchConfig {
    /// Backend selected for [`Backend::Auto`] dispatch.
    pub backend: Backend,
    /// Minimum MSM size before trying an accelerated backend.
    pub min_msm_size: usize,
}

impl Default for DispatchConfig {
    fn default() -> Self {
        Self {
            backend: Backend::Cpu,
            min_msm_size: DEFAULT_MIN_MSM,
        }
    }
}

/// Default minimum size for a medium MSM to be considered batchable.
pub const DEFAULT_MIN_BATCH_MSM: usize = 1024;

/// Default number of medium MSMs required before a batch is worth scheduling.
pub const DEFAULT_MIN_BATCH_ITEMS: usize = 2;

/// Default total points required before a medium-MSM batch is worth scheduling.
pub const DEFAULT_MIN_BATCH_POINTS: usize = DEFAULT_MIN_MSM;

/// Policy knobs for planning MSM execution before handing work to a backend.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MsmBatchConfig {
    /// Backend requested for accelerated work.
    pub backend: Backend,
    /// MSMs at or above this size are considered standalone acceleration work.
    pub min_single_msm_size: usize,
    /// MSMs at or above this size but below `min_single_msm_size` can be batched.
    pub min_batch_msm_size: usize,
    /// Minimum number of medium MSMs required to keep a batch candidate.
    pub min_batch_items: usize,
    /// Minimum total points required to keep a batch candidate.
    pub min_batch_points: usize,
}

impl Default for MsmBatchConfig {
    fn default() -> Self {
        Self {
            backend: Backend::Cpu,
            min_single_msm_size: DEFAULT_MIN_MSM,
            min_batch_msm_size: DEFAULT_MIN_BATCH_MSM,
            min_batch_items: DEFAULT_MIN_BATCH_ITEMS,
            min_batch_points: DEFAULT_MIN_BATCH_POINTS,
        }
    }
}

impl From<DispatchConfig> for MsmBatchConfig {
    fn from(config: DispatchConfig) -> Self {
        Self {
            backend: config.backend,
            min_single_msm_size: config.min_msm_size,
            ..Self::default()
        }
    }
}

/// Schedule decision for one MSM request in an [`MsmBatchPlan`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MsmScheduleDecision {
    /// Run this MSM on the caller's CPU path.
    Cpu,
    /// Accumulate this medium MSM with compatible batch work.
    Batch,
    /// Send this large MSM to the requested backend as standalone work.
    Immediate,
}

/// Aggregate counts for an MSM schedule plan.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct MsmScheduleSummary {
    /// Number of MSMs assigned to CPU execution.
    pub cpu_items: u64,
    /// Total points assigned to CPU execution.
    pub cpu_points: u64,
    /// Number of MSMs assigned to medium-MSM batching.
    pub batch_items: u64,
    /// Total points assigned to medium-MSM batching.
    pub batch_points: u64,
    /// Number of MSMs assigned to standalone accelerated execution.
    pub immediate_items: u64,
    /// Total points assigned to standalone accelerated execution.
    pub immediate_points: u64,
}

impl MsmScheduleSummary {
    fn record(&mut self, decision: MsmScheduleDecision, points: usize) {
        let points = points as u64;
        match decision {
            MsmScheduleDecision::Cpu => {
                self.cpu_items = self.cpu_items.saturating_add(1);
                self.cpu_points = self.cpu_points.saturating_add(points);
            }
            MsmScheduleDecision::Batch => {
                self.batch_items = self.batch_items.saturating_add(1);
                self.batch_points = self.batch_points.saturating_add(points);
            }
            MsmScheduleDecision::Immediate => {
                self.immediate_items = self.immediate_items.saturating_add(1);
                self.immediate_points = self.immediate_points.saturating_add(points);
            }
        }
    }
}

/// Policy-only MSM schedule plan.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct MsmBatchPlan {
    /// Per-request decisions matching the input order.
    pub decisions: Vec<MsmScheduleDecision>,
    /// Aggregate counts for logging, metrics, and threshold tuning.
    pub summary: MsmScheduleSummary,
}

/// Plans how a set of pending MSM sizes should be scheduled.
///
/// This does not inspect backend availability or execute work. It gives Ragu,
/// Halo2, and Zebra integration points a deterministic way to decide whether
/// small MSMs stay on CPU, medium MSMs are worth batching, and large MSMs
/// should be dispatched immediately.
pub fn plan_msm_schedule(request_points: &[usize], config: MsmBatchConfig) -> MsmBatchPlan {
    let mut decisions = Vec::with_capacity(request_points.len());
    let acceleration_requested = config.backend != Backend::Cpu;
    let min_single_msm_size = config.min_single_msm_size.max(1);
    let min_batch_msm_size = config.min_batch_msm_size.max(1);

    let mut batch_items = 0_u64;
    let mut batch_points = 0_u64;

    for &points in request_points {
        let decision = if !acceleration_requested || points == 0 {
            MsmScheduleDecision::Cpu
        } else if points >= min_single_msm_size {
            MsmScheduleDecision::Immediate
        } else if points >= min_batch_msm_size {
            batch_items = batch_items.saturating_add(1);
            batch_points = batch_points.saturating_add(points as u64);
            MsmScheduleDecision::Batch
        } else {
            MsmScheduleDecision::Cpu
        };

        decisions.push(decision);
    }

    if batch_items < config.min_batch_items as u64 || batch_points < config.min_batch_points as u64
    {
        for decision in &mut decisions {
            if *decision == MsmScheduleDecision::Batch {
                *decision = MsmScheduleDecision::Cpu;
            }
        }
    }

    let mut summary = MsmScheduleSummary::default();
    for (&decision, &points) in decisions.iter().zip(request_points.iter()) {
        summary.record(decision, points);
    }

    MsmBatchPlan { decisions, summary }
}

/// Per-thread acceleration dispatch counters.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct DispatchStats {
    /// Number of MSMs that reached an acceleration candidate path.
    pub msm_candidates: u64,
    /// Total bases/scalars in candidate MSMs.
    pub msm_candidate_points: u64,
    /// Number of candidate MSMs served by the facade without CPU fallback.
    pub msm_successes: u64,
    /// Number of candidate MSMs that fell back to CPU.
    pub msm_fallbacks: u64,
}

thread_local! {
    static DISPATCH_CONFIG_OVERRIDE: Cell<Option<DispatchConfig>> = const { Cell::new(None) };
    static DISPATCH_STATS: Cell<DispatchStats> = const { Cell::new(DispatchStats {
        msm_candidates: 0,
        msm_candidate_points: 0,
        msm_successes: 0,
        msm_fallbacks: 0,
    }) };
}

/// Error returned by acceleration dispatch.
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum AccelError {
    /// The requested curve is not supported by this facade.
    #[error("unsupported curve")]
    UnsupportedCurve,
    /// The requested backend is not compiled in or not supported yet.
    #[error("unsupported backend")]
    UnsupportedBackend,
    /// The requested backend is compiled in but unavailable at runtime.
    #[error("backend unavailable")]
    BackendUnavailable,
    /// Scalar and base slices must have the same length.
    #[error("scalar and base length mismatch")]
    LengthMismatch,
    /// The backend returned an invalid point.
    #[error("invalid point")]
    InvalidPoint,
    /// The backend returned an FFI-level error.
    #[error("ffi error: {0}")]
    FfiError(String),
}

/// Returns whether `backend` can be used in this build.
pub fn backend_available(backend: Backend) -> bool {
    match backend {
        Backend::Cpu | Backend::Auto => true,
        Backend::Cuda => cuda_available(),
        Backend::Avx512 => avx512_available(),
    }
}

/// Returns whether the current CPU reports the AVX-512 IFMA feature.
///
/// This is diagnostic only. Until a real AVX-512 MSM implementation is wired
/// in, [`Backend::Avx512`] still reports unavailable and returns
/// [`AccelError::UnsupportedBackend`].
pub fn avx512_runtime_detected() -> bool {
    #[cfg(target_arch = "x86_64")]
    {
        std::arch::is_x86_feature_detected!("avx512ifma")
    }

    #[cfg(not(target_arch = "x86_64"))]
    {
        false
    }
}

/// Runs a deterministic CPU-vs-backend MSM self-test for `backend`.
///
/// [`Backend::Cpu`] and CPU-resolved [`Backend::Auto`] are allowed to pass this
/// generic self-test. Use [`accelerated_backend_self_test`] when the caller
/// specifically needs proof that a non-CPU backend is available and coherent.
pub fn backend_self_test(backend: Backend) -> bool {
    let backend = match backend {
        Backend::Auto => best_available_backend(),
        selected => selected,
    };

    if !backend_available(backend) {
        return false;
    }

    pallas_backend_self_test(backend) && vesta_backend_self_test(backend)
}

/// Returns true only if a non-CPU backend is available and passes self-tests.
pub fn accelerated_backend_self_test(backend: Backend) -> bool {
    match backend {
        Backend::Cpu => false,
        Backend::Cuda | Backend::Avx512 => backend_self_test(backend),
        Backend::Auto => backend_self_test(Backend::Cuda) || backend_self_test(Backend::Avx512),
    }
}

/// Selects the requested backend from `ZCASH_ACCEL`.
///
/// Supported values are `off`, `cpu`, `cuda`, `avx512`, and `auto`. Unknown
/// values are treated as `cpu`.
pub fn selected_backend() -> Backend {
    match std::env::var("ZCASH_ACCEL") {
        Ok(value) => match value.to_ascii_lowercase().as_str() {
            "off" | "cpu" => Backend::Cpu,
            "cuda" => Backend::Cuda,
            "avx512" => Backend::Avx512,
            "auto" => Backend::Auto,
            _ => Backend::Cpu,
        },
        Err(_) => Backend::Cpu,
    }
}

/// Returns the effective acceleration dispatch config for this thread.
///
/// Scoped overrides installed with [`with_dispatch_config`] take precedence
/// over environment variables. Without an override, this reads `ZCASH_ACCEL`
/// and `ZCASH_ACCEL_MIN_MSM`.
pub fn dispatch_config() -> DispatchConfig {
    DISPATCH_CONFIG_OVERRIDE
        .with(Cell::get)
        .unwrap_or_else(env_dispatch_config)
}

/// Returns the effective minimum MSM size for this thread.
pub fn min_msm_size() -> usize {
    dispatch_config().min_msm_size
}

/// Runs `f` with a thread-local acceleration dispatch override.
///
/// This is intended for verifier/prover integrations that need explicit
/// runtime config without mutating process-wide environment variables.
pub fn with_dispatch_config<R>(config: DispatchConfig, f: impl FnOnce() -> R) -> R {
    struct ResetDispatchConfig(Option<DispatchConfig>);

    impl Drop for ResetDispatchConfig {
        fn drop(&mut self) {
            DISPATCH_CONFIG_OVERRIDE.with(|cell| cell.set(self.0));
        }
    }

    let previous = DISPATCH_CONFIG_OVERRIDE.with(|cell| cell.replace(Some(config)));
    let reset = ResetDispatchConfig(previous);
    let result = f();
    drop(reset);
    result
}

/// Records that one MSM reached an acceleration candidate path.
pub fn record_msm_candidate(points: usize) {
    DISPATCH_STATS.with(|cell| {
        let mut stats = cell.get();
        stats.msm_candidates = stats.msm_candidates.saturating_add(1);
        stats.msm_candidate_points = stats.msm_candidate_points.saturating_add(points as u64);
        cell.set(stats);
    });
}

/// Records that one candidate MSM returned an accelerated facade result.
pub fn record_msm_success() {
    DISPATCH_STATS.with(|cell| {
        let mut stats = cell.get();
        stats.msm_successes = stats.msm_successes.saturating_add(1);
        cell.set(stats);
    });
}

/// Records that one candidate MSM fell back to CPU.
pub fn record_msm_fallback() {
    DISPATCH_STATS.with(|cell| {
        let mut stats = cell.get();
        stats.msm_fallbacks = stats.msm_fallbacks.saturating_add(1);
        cell.set(stats);
    });
}

/// Resets per-thread dispatch stats.
pub fn reset_dispatch_stats() {
    DISPATCH_STATS.with(|cell| cell.set(DispatchStats::default()));
}

/// Returns and resets per-thread dispatch stats.
pub fn take_dispatch_stats() -> DispatchStats {
    DISPATCH_STATS.with(|cell| {
        let stats = cell.get();
        cell.set(DispatchStats::default());
        stats
    })
}

/// Computes a Pallas variable-base MSM.
///
/// In the default build, [`Backend::Auto`] is equivalent to [`Backend::Cpu`].
pub fn msm_pallas(
    scalars: &[pallas::Scalar],
    bases: &[pallas::Affine],
    backend: Backend,
) -> Result<pallas::Point, AccelError> {
    if scalars.len() != bases.len() {
        return Err(AccelError::LengthMismatch);
    }

    match resolve_backend(backend) {
        Backend::Cpu | Backend::Auto => Ok(cpu_msm_pallas(scalars, bases)),
        Backend::Cuda => cuda_msm_pallas(scalars, bases),
        Backend::Avx512 => avx512_msm_pallas(scalars, bases),
    }
}

/// Computes a Vesta variable-base MSM.
///
/// In the default build, [`Backend::Auto`] is equivalent to [`Backend::Cpu`].
pub fn msm_vesta(
    scalars: &[vesta::Scalar],
    bases: &[vesta::Affine],
    backend: Backend,
) -> Result<vesta::Point, AccelError> {
    if scalars.len() != bases.len() {
        return Err(AccelError::LengthMismatch);
    }

    match resolve_backend(backend) {
        Backend::Cpu | Backend::Auto => Ok(cpu_msm_vesta(scalars, bases)),
        Backend::Cuda => cuda_msm_vesta(scalars, bases),
        Backend::Avx512 => avx512_msm_vesta(scalars, bases),
    }
}

/// Attempts to compute a generic Pasta MSM from type-erased collected inputs.
///
/// This is intended for generic downstream code such as Ragu's `mul<C>`, where
/// the caller has already collected `Vec<C::Scalar>` and `Vec<C>` but cannot
/// name `C` as Pallas or Vesta without specialization. Returns `Ok(None)` when
/// `C` is not one of the supported Pasta affine types.
pub fn try_msm<C>(
    scalars: &dyn Any,
    bases: &dyn Any,
    backend: Backend,
) -> Result<Option<C::Curve>, AccelError>
where
    C: CurveAffine + 'static,
    C::Curve: Clone + 'static,
{
    if let (Some(scalars), Some(bases)) = (
        scalars.downcast_ref::<Vec<pallas::Scalar>>(),
        bases.downcast_ref::<Vec<pallas::Affine>>(),
    ) {
        let result = msm_pallas(scalars, bases, backend)?;
        return downcast_curve::<C, _>(&result)
            .ok_or(AccelError::InvalidPoint)
            .map(Some);
    }

    if let (Some(scalars), Some(bases)) = (
        scalars.downcast_ref::<Vec<vesta::Scalar>>(),
        bases.downcast_ref::<Vec<vesta::Affine>>(),
    ) {
        let result = msm_vesta(scalars, bases, backend)?;
        return downcast_curve::<C, _>(&result)
            .ok_or(AccelError::InvalidPoint)
            .map(Some);
    }

    Ok(None)
}

fn cpu_msm_pallas(scalars: &[pallas::Scalar], bases: &[pallas::Affine]) -> pallas::Point {
    scalars
        .iter()
        .zip(bases.iter())
        .fold(pallas::Point::identity(), |acc, (scalar, base)| {
            acc + (*base * *scalar)
        })
}

fn pallas_backend_self_test(backend: Backend) -> bool {
    let bases = [
        pallas::Point::generator().to_affine(),
        (pallas::Point::generator() * pallas::Scalar::from(7)).to_affine(),
        pallas::Point::identity().to_affine(),
        (pallas::Point::generator() * pallas::Scalar::from(19)).to_affine(),
    ];
    let scalars = [
        pallas::Scalar::from(3),
        -pallas::Scalar::from(11),
        pallas::Scalar::from(23),
        pallas::Scalar::ZERO,
    ];

    msm_pallas(&scalars, &bases, Backend::Cpu)
        .and_then(|expected| msm_pallas(&scalars, &bases, backend).map(|actual| actual == expected))
        .unwrap_or(false)
}

fn vesta_backend_self_test(backend: Backend) -> bool {
    let bases = [
        vesta::Point::generator().to_affine(),
        (vesta::Point::generator() * vesta::Scalar::from(7)).to_affine(),
        vesta::Point::identity().to_affine(),
        (vesta::Point::generator() * vesta::Scalar::from(19)).to_affine(),
    ];
    let scalars = [
        vesta::Scalar::from(3),
        -vesta::Scalar::from(11),
        vesta::Scalar::from(23),
        vesta::Scalar::ZERO,
    ];

    msm_vesta(&scalars, &bases, Backend::Cpu)
        .and_then(|expected| msm_vesta(&scalars, &bases, backend).map(|actual| actual == expected))
        .unwrap_or(false)
}

fn resolve_backend(backend: Backend) -> Backend {
    match backend {
        Backend::Auto => match dispatch_config().backend {
            Backend::Auto => best_available_backend(),
            selected => selected,
        },
        selected => selected,
    }
}

fn env_dispatch_config() -> DispatchConfig {
    DispatchConfig {
        backend: selected_backend(),
        min_msm_size: env_min_msm_size(),
    }
}

fn env_min_msm_size() -> usize {
    std::env::var("ZCASH_ACCEL_MIN_MSM")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(DEFAULT_MIN_MSM)
}

fn best_available_backend() -> Backend {
    if backend_available(Backend::Cuda) {
        Backend::Cuda
    } else if backend_available(Backend::Avx512) {
        Backend::Avx512
    } else {
        Backend::Cpu
    }
}

fn avx512_available() -> bool {
    false
}

fn cuda_available() -> bool {
    #[cfg(feature = "cuda")]
    {
        pasta_msm::cuda_available_runtime()
    }

    #[cfg(not(feature = "cuda"))]
    {
        false
    }
}

fn cuda_msm_pallas(
    scalars: &[pallas::Scalar],
    bases: &[pallas::Affine],
) -> Result<pallas::Point, AccelError> {
    #[cfg(feature = "cuda")]
    {
        if !cuda_available() {
            return Err(AccelError::BackendUnavailable);
        }

        pasta_msm::try_pallas(bases, scalars).map_err(map_pasta_msm_error)
    }

    #[cfg(not(feature = "cuda"))]
    {
        let _ = (scalars, bases);
        Err(AccelError::UnsupportedBackend)
    }
}

fn cuda_msm_vesta(
    scalars: &[vesta::Scalar],
    bases: &[vesta::Affine],
) -> Result<vesta::Point, AccelError> {
    #[cfg(feature = "cuda")]
    {
        if !cuda_available() {
            return Err(AccelError::BackendUnavailable);
        }

        pasta_msm::try_vesta(bases, scalars).map_err(map_pasta_msm_error)
    }

    #[cfg(not(feature = "cuda"))]
    {
        let _ = (scalars, bases);
        Err(AccelError::UnsupportedBackend)
    }
}

#[cfg(feature = "cuda")]
fn map_pasta_msm_error(error: pasta_msm::Error) -> AccelError {
    match error {
        pasta_msm::Error::LengthMismatch => AccelError::LengthMismatch,
        pasta_msm::Error::Cuda(error) => AccelError::FfiError(error),
    }
}

fn avx512_msm_pallas(
    scalars: &[pallas::Scalar],
    bases: &[pallas::Affine],
) -> Result<pallas::Point, AccelError> {
    let _ = (scalars, bases);
    Err(AccelError::UnsupportedBackend)
}

fn avx512_msm_vesta(
    scalars: &[vesta::Scalar],
    bases: &[vesta::Affine],
) -> Result<vesta::Point, AccelError> {
    let _ = (scalars, bases);
    Err(AccelError::UnsupportedBackend)
}

fn downcast_curve<C, P>(point: &P) -> Option<C::Curve>
where
    C: CurveAffine + 'static,
    C::Curve: Clone + 'static,
    P: 'static,
{
    (point as &dyn Any).downcast_ref::<C::Curve>().cloned()
}

fn cpu_msm_vesta(scalars: &[vesta::Scalar], bases: &[vesta::Affine]) -> vesta::Point {
    scalars
        .iter()
        .zip(bases.iter())
        .fold(vesta::Point::identity(), |acc, (scalar, base)| {
            acc + (*base * *scalar)
        })
}
