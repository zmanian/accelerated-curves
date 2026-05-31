//! C-compatible Pasta field and point boundary types.
//!
//! The CUDA backend is intentionally not wired in yet. These types provide a
//! stable, checked conversion layer for future GPU adapters so call sites can
//! validate field canonicality, curve membership, and point-at-infinity flags
//! before any foreign code is involved.

use pasta_curves::{
    arithmetic::{Coordinates, CurveAffine, CurveExt},
    group::ff::PrimeField,
    pallas, vesta,
};

use crate::AccelError;

/// Canonical 255-bit Pasta field element encoded as four little-endian limbs.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct FfiFieldElement {
    pub limbs: [u64; 4],
}

/// Affine Pasta point encoded as `(x, y, infinity)`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct FfiAffinePoint {
    pub x: FfiFieldElement,
    pub y: FfiFieldElement,
    pub infinity: u8,
}

/// Projective Pasta point encoded as `(x, y, z, infinity)`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct FfiProjectivePoint {
    pub x: FfiFieldElement,
    pub y: FfiFieldElement,
    pub z: FfiFieldElement,
    pub infinity: u8,
}

impl FfiFieldElement {
    /// Converts a Pasta field element into little-endian limbs.
    pub fn from_field<F>(field: &F) -> Self
    where
        F: PrimeField,
    {
        let repr = field.to_repr();
        let bytes = repr.as_ref();
        debug_assert_eq!(bytes.len(), 32);

        let mut limbs = [0_u64; 4];
        for (limb, chunk) in limbs.iter_mut().zip(bytes.chunks_exact(8)) {
            *limb = u64::from_le_bytes(chunk.try_into().expect("chunk is 8 bytes"));
        }

        Self { limbs }
    }

    /// Parses a canonical Pasta field element from little-endian limbs.
    pub fn to_field<F>(&self) -> Result<F, AccelError>
    where
        F: PrimeField,
    {
        let mut bytes = [0_u8; 32];
        for (chunk, limb) in bytes.chunks_exact_mut(8).zip(self.limbs) {
            chunk.copy_from_slice(&limb.to_le_bytes());
        }

        let mut repr = F::Repr::default();
        if repr.as_ref().len() != bytes.len() {
            return Err(AccelError::InvalidPoint);
        }
        repr.as_mut().copy_from_slice(&bytes);

        Option::<F>::from(F::from_repr(repr)).ok_or(AccelError::InvalidPoint)
    }
}

impl FfiAffinePoint {
    /// Converts a Pallas affine point into the FFI boundary layout.
    pub fn from_pallas_affine(point: &pallas::Affine) -> Self {
        affine_to_ffi::<pallas::Affine>(point)
    }

    /// Parses a checked Pallas affine point from the FFI boundary layout.
    pub fn to_pallas_affine(&self) -> Result<pallas::Affine, AccelError> {
        ffi_to_affine::<pallas::Affine>(self)
    }

    /// Converts a Vesta affine point into the FFI boundary layout.
    pub fn from_vesta_affine(point: &vesta::Affine) -> Self {
        affine_to_ffi::<vesta::Affine>(point)
    }

    /// Parses a checked Vesta affine point from the FFI boundary layout.
    pub fn to_vesta_affine(&self) -> Result<vesta::Affine, AccelError> {
        ffi_to_affine::<vesta::Affine>(self)
    }
}

impl FfiProjectivePoint {
    /// Converts a Pallas projective point into the FFI boundary layout.
    pub fn from_pallas_point(point: &pallas::Point) -> Self {
        projective_to_ffi::<pallas::Point>(point)
    }

    /// Parses a checked Pallas projective point from the FFI boundary layout.
    pub fn to_pallas_point(&self) -> Result<pallas::Point, AccelError> {
        ffi_to_projective::<pallas::Point>(self)
    }

    /// Converts a Vesta projective point into the FFI boundary layout.
    pub fn from_vesta_point(point: &vesta::Point) -> Self {
        projective_to_ffi::<vesta::Point>(point)
    }

    /// Parses a checked Vesta projective point from the FFI boundary layout.
    pub fn to_vesta_point(&self) -> Result<vesta::Point, AccelError> {
        ffi_to_projective::<vesta::Point>(self)
    }
}

fn affine_to_ffi<C>(point: &C) -> FfiAffinePoint
where
    C: CurveAffine,
{
    let coordinates: Option<Coordinates<C>> = Option::from(point.coordinates());

    match coordinates {
        Some(coords) => FfiAffinePoint {
            x: FfiFieldElement::from_field::<C::Base>(coords.x()),
            y: FfiFieldElement::from_field::<C::Base>(coords.y()),
            infinity: 0,
        },
        None => FfiAffinePoint {
            infinity: 1,
            ..FfiAffinePoint::default()
        },
    }
}

fn ffi_to_affine<C>(point: &FfiAffinePoint) -> Result<C, AccelError>
where
    C: CurveAffine,
{
    if point.infinity != 0 {
        return Ok(C::identity());
    }

    let x = point.x.to_field::<C::Base>()?;
    let y = point.y.to_field::<C::Base>()?;

    Option::<C>::from(C::from_xy(x, y)).ok_or(AccelError::InvalidPoint)
}

fn projective_to_ffi<C>(point: &C) -> FfiProjectivePoint
where
    C: CurveExt,
{
    let (x, y, z) = point.jacobian_coordinates();

    FfiProjectivePoint {
        x: FfiFieldElement::from_field::<C::Base>(&x),
        y: FfiFieldElement::from_field::<C::Base>(&y),
        z: FfiFieldElement::from_field::<C::Base>(&z),
        infinity: u8::from(bool::from(point.is_identity())),
    }
}

fn ffi_to_projective<C>(point: &FfiProjectivePoint) -> Result<C, AccelError>
where
    C: CurveExt,
{
    if point.infinity != 0 {
        return Ok(C::identity());
    }

    let x = point.x.to_field::<C::Base>()?;
    let y = point.y.to_field::<C::Base>()?;
    let z = point.z.to_field::<C::Base>()?;

    Option::<C>::from(C::new_jacobian(x, y, z)).ok_or(AccelError::InvalidPoint)
}
