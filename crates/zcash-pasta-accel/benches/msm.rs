use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};
use pasta_curves::{
    group::{Curve, Group},
    pallas, vesta,
};
use rand::{RngCore, SeedableRng};
use rand_chacha::ChaCha20Rng;
use zcash_pasta_accel::{msm_pallas, msm_vesta, Backend};

const SIZES: &[usize] = &[
    1, 2, 4, 8, 16, 32, 64, 128, 256, 512, 1024, 2048, 4096, 8192, 16384, 32768, 65536, 131072,
];

fn pallas_inputs(size: usize) -> (Vec<pallas::Scalar>, Vec<pallas::Affine>) {
    let mut rng = ChaCha20Rng::seed_from_u64(0xacc3_1001);
    let scalars = (0..size)
        .map(|_| pallas::Scalar::from(rng.next_u64()))
        .collect::<Vec<_>>();
    let bases = (0..size)
        .map(|_| (pallas::Point::generator() * pallas::Scalar::from(rng.next_u64())).to_affine())
        .collect::<Vec<_>>();
    (scalars, bases)
}

fn vesta_inputs(size: usize) -> (Vec<vesta::Scalar>, Vec<vesta::Affine>) {
    let mut rng = ChaCha20Rng::seed_from_u64(0xacc3_1002);
    let scalars = (0..size)
        .map(|_| vesta::Scalar::from(rng.next_u64()))
        .collect::<Vec<_>>();
    let bases = (0..size)
        .map(|_| (vesta::Point::generator() * vesta::Scalar::from(rng.next_u64())).to_affine())
        .collect::<Vec<_>>();
    (scalars, bases)
}

fn bench_msm(c: &mut Criterion) {
    let mut group = c.benchmark_group("msm");

    for &size in SIZES {
        let (scalars, bases) = pallas_inputs(size);
        group.bench_with_input(BenchmarkId::new("pallas_cpu", size), &size, |b, _| {
            b.iter(|| msm_pallas(&scalars, &bases, Backend::Cpu).unwrap());
        });
        group.bench_with_input(BenchmarkId::new("pallas_avx512", size), &size, |b, _| {
            b.iter(|| {
                msm_pallas(&scalars, &bases, Backend::Avx512)
                    .unwrap_or_else(|_| msm_pallas(&scalars, &bases, Backend::Cpu).unwrap())
            });
        });

        let (scalars, bases) = vesta_inputs(size);
        group.bench_with_input(BenchmarkId::new("vesta_cpu", size), &size, |b, _| {
            b.iter(|| msm_vesta(&scalars, &bases, Backend::Cpu).unwrap());
        });
        group.bench_with_input(BenchmarkId::new("vesta_avx512", size), &size, |b, _| {
            b.iter(|| {
                msm_vesta(&scalars, &bases, Backend::Avx512)
                    .unwrap_or_else(|_| msm_vesta(&scalars, &bases, Backend::Cpu).unwrap())
            });
        });
    }

    group.finish();
}

criterion_group!(benches, bench_msm);
criterion_main!(benches);
