#![no_main]

use libfuzzer_sys::fuzz_target;
use pasta_curves::{
    group::{Curve, Group},
    pallas, vesta,
};
use zcash_pasta_accel::{
    backend_available, msm_pallas, msm_vesta, with_dispatch_config, AccelError, Backend,
    DispatchConfig,
};

const MAX_POINTS: usize = 128;

fuzz_target!(|data: &[u8]| {
    if data.is_empty() {
        return;
    }

    let options = data[0];
    let backend = match (options >> 1) & 0b11 {
        0 => Backend::Cpu,
        1 => Backend::Auto,
        2 => Backend::Cuda,
        _ => Backend::Avx512,
    };
    let truncate_bases = options & 0b1 == 1;

    if options & 0b1000 != 0 {
        fuzz_vesta(&data[1..], backend, truncate_bases);
    } else {
        fuzz_pallas(&data[1..], backend, truncate_bases);
    }
});

fn fuzz_pallas(data: &[u8], backend: Backend, truncate_bases: bool) {
    let generator = pallas::Point::generator();
    let mut scalars = Vec::new();
    let mut bases = Vec::new();

    for chunk in data.chunks_exact(16).take(MAX_POINTS) {
        let scalar = u64_from_le(&chunk[..8]);
        let base = u64_from_le(&chunk[8..]);

        scalars.push(scalar_from_seed_pallas(scalar));
        bases.push(base_from_seed_pallas(generator, base));
    }

    if truncate_bases && !bases.is_empty() {
        bases.pop();
    }

    let result = with_cpu_resolved_auto(backend, || msm_pallas(&scalars, &bases, backend));

    if scalars.len() != bases.len() {
        assert_eq!(result, Err(AccelError::LengthMismatch));
        return;
    }

    let expected = expected_pallas(&scalars, &bases);
    assert_backend_result(backend, result.map(|point| point == expected));
}

fn fuzz_vesta(data: &[u8], backend: Backend, truncate_bases: bool) {
    let generator = vesta::Point::generator();
    let mut scalars = Vec::new();
    let mut bases = Vec::new();

    for chunk in data.chunks_exact(16).take(MAX_POINTS) {
        let scalar = u64_from_le(&chunk[..8]);
        let base = u64_from_le(&chunk[8..]);

        scalars.push(scalar_from_seed_vesta(scalar));
        bases.push(base_from_seed_vesta(generator, base));
    }

    if truncate_bases && !bases.is_empty() {
        bases.pop();
    }

    let result = with_cpu_resolved_auto(backend, || msm_vesta(&scalars, &bases, backend));

    if scalars.len() != bases.len() {
        assert_eq!(result, Err(AccelError::LengthMismatch));
        return;
    }

    let expected = expected_vesta(&scalars, &bases);
    assert_backend_result(backend, result.map(|point| point == expected));
}

fn with_cpu_resolved_auto<T>(backend: Backend, f: impl FnOnce() -> T) -> T {
    if backend == Backend::Auto {
        with_dispatch_config(
            DispatchConfig {
                backend: Backend::Cpu,
                min_msm_size: 0,
            },
            f,
        )
    } else {
        f()
    }
}

fn assert_backend_result(backend: Backend, result: Result<bool, AccelError>) {
    match result {
        Ok(matches_cpu) => assert!(matches_cpu),
        Err(AccelError::UnsupportedBackend | AccelError::BackendUnavailable)
            if backend != Backend::Cpu && !backend_available(backend) => {}
        Err(error) => panic!("unexpected MSM error for {backend:?}: {error:?}"),
    }
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

fn scalar_from_seed_pallas(seed: u64) -> pallas::Scalar {
    if seed & 0b1 == 0 {
        pallas::Scalar::from(seed)
    } else {
        -pallas::Scalar::from(seed)
    }
}

fn scalar_from_seed_vesta(seed: u64) -> vesta::Scalar {
    if seed & 0b1 == 0 {
        vesta::Scalar::from(seed)
    } else {
        -vesta::Scalar::from(seed)
    }
}

fn base_from_seed_pallas(generator: pallas::Point, seed: u64) -> pallas::Affine {
    if seed & 0b1 == 0 {
        pallas::Point::identity().to_affine()
    } else {
        (generator * pallas::Scalar::from(seed)).to_affine()
    }
}

fn base_from_seed_vesta(generator: vesta::Point, seed: u64) -> vesta::Affine {
    if seed & 0b1 == 0 {
        vesta::Point::identity().to_affine()
    } else {
        (generator * vesta::Scalar::from(seed)).to_affine()
    }
}

fn u64_from_le(bytes: &[u8]) -> u64 {
    let mut value = [0u8; 8];
    value.copy_from_slice(bytes);
    u64::from_le_bytes(value)
}
