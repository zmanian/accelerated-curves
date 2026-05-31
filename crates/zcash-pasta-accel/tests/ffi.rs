use pasta_curves::{
    group::{ff::Field, prime::PrimeCurveAffine, Curve, Group},
    pallas, vesta, Fp, Fq,
};
use std::mem::{align_of, size_of};
use zcash_pasta_accel::{
    ffi::{FfiAffinePoint, FfiFieldElement, FfiProjectivePoint},
    AccelError,
};

fn pallas_sample_point() -> pallas::Affine {
    (pallas::Point::generator() * pallas::Scalar::from(17)).to_affine()
}

fn vesta_sample_point() -> vesta::Affine {
    (vesta::Point::generator() * vesta::Scalar::from(29)).to_affine()
}

#[test]
fn ffi_layout_is_c_compatible_and_limb_aligned() {
    assert_eq!(size_of::<FfiFieldElement>(), 32);
    assert_eq!(align_of::<FfiFieldElement>(), align_of::<u64>());
    assert!(size_of::<FfiAffinePoint>() >= 65);
    assert!(size_of::<FfiProjectivePoint>() >= 97);
}

#[test]
fn field_elements_round_trip_as_little_endian_limbs() {
    let fq = Fq::from(0x1122_3344_5566_7788);
    let fq_ffi = FfiFieldElement::from_field(&fq);
    assert_eq!(fq_ffi.limbs[0], 0x1122_3344_5566_7788);
    assert_eq!(fq_ffi.to_field::<Fq>().expect("Fq is canonical"), fq);

    let fp = Fp::from(0x8877_6655_4433_2211);
    let fp_ffi = FfiFieldElement::from_field(&fp);
    assert_eq!(fp_ffi.limbs[0], 0x8877_6655_4433_2211);
    assert_eq!(fp_ffi.to_field::<Fp>().expect("Fp is canonical"), fp);
}

#[test]
fn non_canonical_field_limbs_are_rejected() {
    let element = FfiFieldElement {
        limbs: [u64::MAX; 4],
    };

    assert_eq!(element.to_field::<Fq>(), Err(AccelError::InvalidPoint));
    assert_eq!(element.to_field::<Fp>(), Err(AccelError::InvalidPoint));
}

#[test]
fn pallas_affine_points_round_trip_with_infinity_flag() {
    let point = pallas_sample_point();
    let ffi = FfiAffinePoint::from_pallas_affine(&point);

    assert_eq!(ffi.infinity, 0);
    assert_eq!(ffi.to_pallas_affine().expect("point remains valid"), point);

    let identity = FfiAffinePoint::from_pallas_affine(&pallas::Affine::identity());
    assert_eq!(identity.infinity, 1);
    assert_eq!(
        identity.to_pallas_affine().expect("identity remains valid"),
        pallas::Affine::identity()
    );
}

#[test]
fn vesta_affine_points_round_trip_with_infinity_flag() {
    let point = vesta_sample_point();
    let ffi = FfiAffinePoint::from_vesta_affine(&point);

    assert_eq!(ffi.infinity, 0);
    assert_eq!(ffi.to_vesta_affine().expect("point remains valid"), point);

    let identity = FfiAffinePoint::from_vesta_affine(&vesta::Affine::identity());
    assert_eq!(identity.infinity, 1);
    assert_eq!(
        identity.to_vesta_affine().expect("identity remains valid"),
        vesta::Affine::identity()
    );
}

#[test]
fn invalid_affine_coordinates_are_rejected() {
    let mut point = FfiAffinePoint::from_pallas_affine(&pallas_sample_point());
    point.y = FfiFieldElement::from_field(&Fp::ZERO);

    assert_eq!(point.to_pallas_affine(), Err(AccelError::InvalidPoint));

    let mut point = FfiAffinePoint::from_vesta_affine(&vesta_sample_point());
    point.y = FfiFieldElement::from_field(&Fq::ZERO);

    assert_eq!(point.to_vesta_affine(), Err(AccelError::InvalidPoint));
}

#[test]
fn projective_points_round_trip_through_affine_boundary() {
    let point = pallas::Point::generator() * pallas::Scalar::from(41);
    let ffi = FfiProjectivePoint::from_pallas_point(&point);

    assert_eq!(
        ffi.to_pallas_point().expect("projective point is valid"),
        point
    );

    let point = vesta::Point::generator() * vesta::Scalar::from(43);
    let ffi = FfiProjectivePoint::from_vesta_point(&point);

    assert_eq!(
        ffi.to_vesta_point().expect("projective point is valid"),
        point
    );
}
