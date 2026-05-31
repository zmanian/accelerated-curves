//! Optional acceleration facade for Pasta-curve MSM-heavy workloads.
//!
//! The default implementation is deliberately conservative: it exposes the
//! backend-selection API that downstream crates can integrate against, but only
//! the pure Rust CPU path is available unless future feature-gated backends are
//! added.

use core::any::Any;
use std::cell::Cell;

use pasta_curves::{
    arithmetic::CurveAffine,
    group::{ff::Field, prime::PrimeCurveAffine, Curve, Group},
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
        pallas::Affine::identity(),
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
        vesta::Affine::identity(),
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
    #[cfg(all(feature = "avx512", target_arch = "x86_64"))]
    {
        std::arch::is_x86_feature_detected!("avx512ifma")
    }

    #[cfg(not(all(feature = "avx512", target_arch = "x86_64")))]
    {
        false
    }
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
    if avx512_available() {
        Ok(cpu_msm_pallas(scalars, bases))
    } else {
        Err(AccelError::UnsupportedBackend)
    }
}

fn avx512_msm_vesta(
    scalars: &[vesta::Scalar],
    bases: &[vesta::Affine],
) -> Result<vesta::Point, AccelError> {
    if avx512_available() {
        Ok(cpu_msm_vesta(scalars, bases))
    } else {
        Err(AccelError::UnsupportedBackend)
    }
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
