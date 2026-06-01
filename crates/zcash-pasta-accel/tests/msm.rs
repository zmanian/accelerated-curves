use pasta_curves::{
    group::{ff::Field, prime::PrimeCurveAffine, Curve, Group},
    pallas, vesta,
};
use proptest::prelude::*;
use rand::{RngCore, SeedableRng};
use rand_chacha::ChaCha20Rng;
use std::sync::{Mutex, OnceLock};
use zcash_pasta_accel::{
    accelerated_backend_self_test, avx512_runtime_detected, backend_available, backend_self_test,
    dispatch_config, min_msm_size, msm_pallas, msm_vesta, plan_msm_schedule, record_msm_candidate,
    record_msm_fallback, record_msm_success, reset_dispatch_stats, selected_backend,
    take_dispatch_stats, try_msm, with_dispatch_config, AccelError, Backend, DispatchConfig,
    DispatchStats, MsmBatchConfig, MsmScheduleDecision, MsmScheduleSummary,
};

fn env_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(())).lock().unwrap()
}

fn expected_pallas(scalars: &[pallas::Scalar], bases: &[pallas::Affine]) -> pallas::Point {
    scalars
        .iter()
        .zip(bases.iter())
        .fold(pallas::Point::identity(), |acc, (scalar, base)| {
            acc + (*base * *scalar)
        })
}

fn expected_vesta(scalars: &[vesta::Scalar], bases: &[vesta::Affine]) -> vesta::Point {
    scalars
        .iter()
        .zip(bases.iter())
        .fold(vesta::Point::identity(), |acc, (scalar, base)| {
            acc + (*base * *scalar)
        })
}

#[test]
fn cpu_backend_is_available_by_default() {
    assert!(backend_available(Backend::Cpu));
    assert!(backend_available(Backend::Auto));
}

#[test]
fn unavailable_backends_report_unavailable() {
    assert!(!backend_available(Backend::Cuda));
    assert!(!backend_available(Backend::Avx512));
}

#[test]
fn avx512_runtime_detection_does_not_claim_stub_backend_availability() {
    let _ = avx512_runtime_detected();

    assert!(!backend_available(Backend::Avx512));
    assert!(!backend_self_test(Backend::Avx512));
    assert!(!accelerated_backend_self_test(Backend::Avx512));
}

#[test]
fn backend_self_tests_require_available_accelerated_backends() {
    assert!(backend_self_test(Backend::Cpu));
    assert!(backend_self_test(Backend::Auto));
    assert!(!accelerated_backend_self_test(Backend::Cpu));

    for backend in [Backend::Cuda, Backend::Avx512] {
        if backend_available(backend) {
            assert!(backend_self_test(backend));
            assert!(accelerated_backend_self_test(backend));
        } else {
            assert!(!backend_self_test(backend));
            assert!(!accelerated_backend_self_test(backend));
        }
    }
}

#[test]
fn selected_backend_defaults_to_cpu_when_env_is_absent_or_off() {
    let _guard = env_lock();

    std::env::remove_var("ZCASH_ACCEL");
    assert_eq!(selected_backend(), Backend::Cpu);

    std::env::set_var("ZCASH_ACCEL", "off");
    assert_eq!(selected_backend(), Backend::Cpu);

    std::env::remove_var("ZCASH_ACCEL");
}

#[test]
fn selected_backend_parses_supported_env_values_case_insensitively() {
    let _guard = env_lock();

    for (value, expected) in [
        ("cpu", Backend::Cpu),
        ("CUDA", Backend::Cuda),
        ("avx512", Backend::Avx512),
        ("auto", Backend::Auto),
    ] {
        std::env::set_var("ZCASH_ACCEL", value);
        assert_eq!(selected_backend(), expected);
    }

    std::env::set_var("ZCASH_ACCEL", "nonsense");
    assert_eq!(selected_backend(), Backend::Cpu);

    std::env::remove_var("ZCASH_ACCEL");
}

#[test]
fn dispatch_config_override_is_thread_local_and_scoped() {
    let _guard = env_lock();

    std::env::set_var("ZCASH_ACCEL", "cuda");
    std::env::set_var("ZCASH_ACCEL_MIN_MSM", "17");

    assert_eq!(
        dispatch_config(),
        DispatchConfig {
            backend: Backend::Cuda,
            min_msm_size: 17,
        }
    );
    assert_eq!(min_msm_size(), 17);

    with_dispatch_config(
        DispatchConfig {
            backend: Backend::Avx512,
            min_msm_size: 3,
        },
        || {
            assert_eq!(
                dispatch_config(),
                DispatchConfig {
                    backend: Backend::Avx512,
                    min_msm_size: 3,
                }
            );
            assert_eq!(min_msm_size(), 3);

            with_dispatch_config(
                DispatchConfig {
                    backend: Backend::Cpu,
                    min_msm_size: 9,
                },
                || {
                    assert_eq!(
                        dispatch_config(),
                        DispatchConfig {
                            backend: Backend::Cpu,
                            min_msm_size: 9,
                        }
                    );
                    assert_eq!(min_msm_size(), 9);
                },
            );

            assert_eq!(
                dispatch_config(),
                DispatchConfig {
                    backend: Backend::Avx512,
                    min_msm_size: 3,
                }
            );
        },
    );

    assert_eq!(
        dispatch_config(),
        DispatchConfig {
            backend: Backend::Cuda,
            min_msm_size: 17,
        }
    );

    std::env::remove_var("ZCASH_ACCEL");
    std::env::remove_var("ZCASH_ACCEL_MIN_MSM");
}

#[test]
fn dispatch_stats_track_candidates_successes_and_fallbacks() {
    reset_dispatch_stats();
    assert_eq!(take_dispatch_stats(), DispatchStats::default());

    record_msm_candidate(11);
    record_msm_success();
    record_msm_candidate(7);
    record_msm_fallback();

    assert_eq!(
        take_dispatch_stats(),
        DispatchStats {
            msm_candidates: 2,
            msm_candidate_points: 18,
            msm_successes: 1,
            msm_fallbacks: 1,
        }
    );
    assert_eq!(take_dispatch_stats(), DispatchStats::default());
}

#[test]
fn msm_schedule_plan_batches_medium_work_and_keeps_large_work_immediate() {
    let config = MsmBatchConfig {
        backend: Backend::Cuda,
        min_single_msm_size: 4096,
        min_batch_msm_size: 1024,
        min_batch_items: 3,
        min_batch_points: 4096,
    };

    let plan = plan_msm_schedule(&[512, 1024, 1536, 2048, 4096], config);

    assert_eq!(
        plan.decisions,
        vec![
            MsmScheduleDecision::Cpu,
            MsmScheduleDecision::Batch,
            MsmScheduleDecision::Batch,
            MsmScheduleDecision::Batch,
            MsmScheduleDecision::Immediate,
        ]
    );
    assert_eq!(
        plan.summary,
        MsmScheduleSummary {
            cpu_items: 1,
            cpu_points: 512,
            batch_items: 3,
            batch_points: 4608,
            immediate_items: 1,
            immediate_points: 4096,
        }
    );
}

#[test]
fn msm_schedule_plan_demotes_underfilled_batches_to_cpu() {
    let config = MsmBatchConfig {
        backend: Backend::Cuda,
        min_single_msm_size: 4096,
        min_batch_msm_size: 1024,
        min_batch_items: 3,
        min_batch_points: 4096,
    };

    let plan = plan_msm_schedule(&[1024, 1024], config);

    assert_eq!(
        plan.decisions,
        vec![MsmScheduleDecision::Cpu, MsmScheduleDecision::Cpu]
    );
    assert_eq!(
        plan.summary,
        MsmScheduleSummary {
            cpu_items: 2,
            cpu_points: 2048,
            batch_items: 0,
            batch_points: 0,
            immediate_items: 0,
            immediate_points: 0,
        }
    );
}

#[test]
fn msm_schedule_plan_cpu_backend_suppresses_acceleration_candidates() {
    let config = MsmBatchConfig {
        backend: Backend::Cpu,
        min_single_msm_size: 4096,
        min_batch_msm_size: 1024,
        min_batch_items: 2,
        min_batch_points: 2048,
    };

    let plan = plan_msm_schedule(&[1024, 8192], config);

    assert_eq!(
        plan.decisions,
        vec![MsmScheduleDecision::Cpu, MsmScheduleDecision::Cpu]
    );
    assert_eq!(
        plan.summary,
        MsmScheduleSummary {
            cpu_items: 2,
            cpu_points: 9216,
            batch_items: 0,
            batch_points: 0,
            immediate_items: 0,
            immediate_points: 0,
        }
    );
}

#[test]
fn pallas_empty_msm_returns_identity() {
    let result = msm_pallas(&[], &[], Backend::Cpu).expect("empty MSM succeeds");
    assert_eq!(result, pallas::Point::identity());
}

#[test]
fn vesta_empty_msm_returns_identity() {
    let result = msm_vesta(&[], &[], Backend::Cpu).expect("empty MSM succeeds");
    assert_eq!(result, vesta::Point::identity());
}

#[test]
fn pallas_length_mismatch_is_rejected() {
    let scalars = [pallas::Scalar::from(7)];
    let err = msm_pallas(&scalars, &[], Backend::Cpu).expect_err("length mismatch");
    assert_eq!(err, AccelError::LengthMismatch);
}

#[test]
fn vesta_length_mismatch_is_rejected() {
    let scalars = [vesta::Scalar::from(7)];
    let err = msm_vesta(&scalars, &[], Backend::Cpu).expect_err("length mismatch");
    assert_eq!(err, AccelError::LengthMismatch);
}

#[test]
fn pallas_cpu_matches_direct_scalar_multiplication() {
    let bases = [
        pallas::Point::generator().to_affine(),
        (pallas::Point::generator() * pallas::Scalar::from(9)).to_affine(),
        pallas::Affine::identity(),
        pallas::Point::generator().to_affine(),
    ];
    let scalars = [
        pallas::Scalar::from(3),
        pallas::Scalar::from(11),
        pallas::Scalar::from(19),
        pallas::Scalar::ZERO,
    ];

    let result = msm_pallas(&scalars, &bases, Backend::Cpu).expect("CPU MSM succeeds");
    assert_eq!(result, expected_pallas(&scalars, &bases));
}

#[test]
fn vesta_cpu_matches_direct_scalar_multiplication() {
    let bases = [
        vesta::Point::generator().to_affine(),
        (vesta::Point::generator() * vesta::Scalar::from(9)).to_affine(),
        vesta::Affine::identity(),
        vesta::Point::generator().to_affine(),
    ];
    let scalars = [
        vesta::Scalar::from(3),
        vesta::Scalar::from(11),
        vesta::Scalar::from(19),
        vesta::Scalar::ZERO,
    ];

    let result = msm_vesta(&scalars, &bases, Backend::Auto).expect("auto uses CPU");
    assert_eq!(result, expected_vesta(&scalars, &bases));
}

#[test]
fn generic_try_msm_dispatches_pallas() {
    let bases = vec![
        pallas::Point::generator().to_affine(),
        (pallas::Point::generator() * pallas::Scalar::from(13)).to_affine(),
    ];
    let scalars = vec![pallas::Scalar::from(5), pallas::Scalar::from(17)];

    let result = try_msm::<pallas::Affine>(&scalars, &bases, Backend::Cpu)
        .expect("dispatch succeeds")
        .expect("Pallas is supported");

    assert_eq!(result, expected_pallas(&scalars, &bases));
}

#[test]
fn generic_try_msm_dispatches_vesta() {
    let bases = vec![
        vesta::Point::generator().to_affine(),
        (vesta::Point::generator() * vesta::Scalar::from(13)).to_affine(),
    ];
    let scalars = vec![vesta::Scalar::from(5), vesta::Scalar::from(17)];

    let result = try_msm::<vesta::Affine>(&scalars, &bases, Backend::Cpu)
        .expect("dispatch succeeds")
        .expect("Vesta is supported");

    assert_eq!(result, expected_vesta(&scalars, &bases));
}

#[cfg(not(feature = "cuda"))]
#[test]
fn unsupported_backend_returns_error_instead_of_falling_through() {
    let scalars = [pallas::Scalar::from(1)];
    let bases = [pallas::Point::generator().to_affine()];

    let err = msm_pallas(&scalars, &bases, Backend::Cuda).expect_err("cuda unsupported");
    assert_eq!(err, AccelError::UnsupportedBackend);
}

#[cfg(feature = "cuda")]
#[test]
fn cuda_backend_requires_runtime_cuda_availability() {
    let scalars = [vesta::Scalar::from(1)];
    let bases = [vesta::Point::generator().to_affine()];

    if backend_available(Backend::Cuda) {
        let result = msm_vesta(&scalars, &bases, Backend::Cuda).expect("cuda backend succeeds");
        assert_eq!(result, expected_vesta(&scalars, &bases));
    } else {
        let err = msm_vesta(&scalars, &bases, Backend::Cuda).expect_err("cuda unavailable");
        assert_eq!(err, AccelError::BackendUnavailable);
    }
}

#[test]
fn unsupported_avx512_backend_returns_error_without_cpu_acceptance() {
    #[cfg(not(all(feature = "avx512", target_arch = "x86_64")))]
    {
        let scalars = [pallas::Scalar::from(1)];
        let bases = [pallas::Point::generator().to_affine()];

        let err = msm_pallas(&scalars, &bases, Backend::Avx512).expect_err("avx512 unsupported");
        assert_eq!(err, AccelError::UnsupportedBackend);
    }
}

#[test]
fn pallas_randomized_edge_cases_match_direct_scalar_multiplication() {
    let mut rng = ChaCha20Rng::seed_from_u64(0x5eed);
    let generator = pallas::Point::generator();

    let cases: Vec<(Vec<pallas::Scalar>, Vec<pallas::Affine>)> = vec![
        (vec![], vec![]),
        (
            vec![pallas::Scalar::from(rng.next_u64())],
            vec![(generator * pallas::Scalar::from(rng.next_u64())).to_affine()],
        ),
        (
            vec![pallas::Scalar::ZERO; 16],
            (0..16)
                .map(|_| (generator * pallas::Scalar::from(rng.next_u64())).to_affine())
                .collect(),
        ),
        (
            (0..16)
                .map(|_| pallas::Scalar::from(rng.next_u64()))
                .collect(),
            vec![pallas::Affine::identity(); 16],
        ),
        (
            (0..16)
                .map(|_| pallas::Scalar::from(rng.next_u64()))
                .collect(),
            vec![(generator * pallas::Scalar::from(7)).to_affine(); 16],
        ),
        (
            vec![
                -pallas::Scalar::ONE,
                -pallas::Scalar::from(2),
                pallas::Scalar::from(u64::MAX),
            ],
            vec![
                generator.to_affine(),
                (generator * pallas::Scalar::from(17)).to_affine(),
                (generator * pallas::Scalar::from(23)).to_affine(),
            ],
        ),
        (
            (0..512)
                .map(|_| pallas::Scalar::from(rng.next_u64()))
                .collect(),
            (0..512)
                .map(|_| (generator * pallas::Scalar::from(rng.next_u64())).to_affine())
                .collect(),
        ),
    ];

    for (scalars, bases) in cases {
        let result = msm_pallas(&scalars, &bases, Backend::Cpu).expect("CPU MSM succeeds");
        assert_eq!(result, expected_pallas(&scalars, &bases));
    }
}

#[test]
fn vesta_randomized_edge_cases_match_direct_scalar_multiplication() {
    let mut rng = ChaCha20Rng::seed_from_u64(0x5eed);
    let generator = vesta::Point::generator();

    let cases: Vec<(Vec<vesta::Scalar>, Vec<vesta::Affine>)> = vec![
        (vec![], vec![]),
        (
            vec![vesta::Scalar::from(rng.next_u64())],
            vec![(generator * vesta::Scalar::from(rng.next_u64())).to_affine()],
        ),
        (
            vec![vesta::Scalar::ZERO; 16],
            (0..16)
                .map(|_| (generator * vesta::Scalar::from(rng.next_u64())).to_affine())
                .collect(),
        ),
        (
            (0..16)
                .map(|_| vesta::Scalar::from(rng.next_u64()))
                .collect(),
            vec![vesta::Affine::identity(); 16],
        ),
        (
            (0..16)
                .map(|_| vesta::Scalar::from(rng.next_u64()))
                .collect(),
            vec![(generator * vesta::Scalar::from(7)).to_affine(); 16],
        ),
        (
            vec![
                -vesta::Scalar::ONE,
                -vesta::Scalar::from(2),
                vesta::Scalar::from(u64::MAX),
            ],
            vec![
                generator.to_affine(),
                (generator * vesta::Scalar::from(17)).to_affine(),
                (generator * vesta::Scalar::from(23)).to_affine(),
            ],
        ),
        (
            (0..512)
                .map(|_| vesta::Scalar::from(rng.next_u64()))
                .collect(),
            (0..512)
                .map(|_| (generator * vesta::Scalar::from(rng.next_u64())).to_affine())
                .collect(),
        ),
    ];

    for (scalars, bases) in cases {
        let result = msm_vesta(&scalars, &bases, Backend::Cpu).expect("CPU MSM succeeds");
        assert_eq!(result, expected_vesta(&scalars, &bases));
    }
}

proptest! {
    #[test]
    fn pallas_proptest_msm_matches_direct_scalar_multiplication(
        inputs in prop::collection::vec((any::<u64>(), any::<u64>()), 0..64)
    ) {
        let generator = pallas::Point::generator();
        let scalars = inputs
            .iter()
            .map(|(scalar, _)| pallas::Scalar::from(*scalar))
            .collect::<Vec<_>>();
        let bases = inputs
            .iter()
            .map(|(_, base)| (generator * pallas::Scalar::from(*base)).to_affine())
            .collect::<Vec<_>>();

        let result = msm_pallas(&scalars, &bases, Backend::Cpu).expect("CPU MSM succeeds");
        prop_assert_eq!(result, expected_pallas(&scalars, &bases));
    }

    #[test]
    fn vesta_proptest_msm_matches_direct_scalar_multiplication(
        inputs in prop::collection::vec((any::<u64>(), any::<u64>()), 0..64)
    ) {
        let generator = vesta::Point::generator();
        let scalars = inputs
            .iter()
            .map(|(scalar, _)| vesta::Scalar::from(*scalar))
            .collect::<Vec<_>>();
        let bases = inputs
            .iter()
            .map(|(_, base)| (generator * vesta::Scalar::from(*base)).to_affine())
            .collect::<Vec<_>>();

        let result = msm_vesta(&scalars, &bases, Backend::Cpu).expect("CPU MSM succeeds");
        prop_assert_eq!(result, expected_vesta(&scalars, &bases));
    }
}
