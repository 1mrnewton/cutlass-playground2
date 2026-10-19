//! Interpolation primitives shared by every animatable type.

use glam::{Quat, Vec2, Vec3, Vec4};

/// Linear interpolation between two values of the same type.
///
/// Implement this for any property you want to animate. Temporal easing is
/// applied to `t` *before* it reaches `lerp`, so implementations only blend
/// linearly. [`Lerp::offset`] additionally supports spatial motion paths.
pub trait Lerp: Clone {
    fn lerp(&self, other: &Self, t: f32) -> Self;

    /// Offset by a value-space tangent (vector addition), used to build the
    /// control points of a spatial motion path. Defaults to a no-op for types
    /// without a spatial interpretation (e.g. rotations).
    fn offset(&self, _by: &Self) -> Self {
        self.clone()
    }
}

macro_rules! lerp_scalar {
    ($($ty:ty),*) => {$(
        impl Lerp for $ty {
            #[inline]
            fn lerp(&self, other: &Self, t: f32) -> Self {
                (*self as f64 + (*other as f64 - *self as f64) * t as f64) as $ty
            }
            #[inline]
            fn offset(&self, by: &Self) -> Self {
                *self + *by
            }
        }
    )*};
}

lerp_scalar!(f32, f64);

macro_rules! lerp_vec {
    ($($ty:ty),*) => {$(
        impl Lerp for $ty {
            #[inline]
            fn lerp(&self, other: &Self, t: f32) -> Self {
                <$ty>::lerp(*self, *other, t)
            }
            #[inline]
            fn offset(&self, by: &Self) -> Self {
                *self + *by
            }
        }
    )*};
}

lerp_vec!(Vec2, Vec3, Vec4);

impl Lerp for Quat {
    /// Rotations blend with spherical interpolation so animation arcs stay
    /// natural regardless of the angle between keyframes.
    #[inline]
    fn lerp(&self, other: &Self, t: f32) -> Self {
        self.slerp(*other, t)
    }
}

/// Evaluate a cubic Bézier `p0 → p3` with control points `p1`, `p2` at `t`,
/// using De Casteljau's algorithm — generic over any [`Lerp`] type, so the same
/// routine drives spatial motion paths (positions) and path morphs (vertices).
pub fn cubic_bezier<T: Lerp>(p0: &T, p1: &T, p2: &T, p3: &T, t: f32) -> T {
    let a = p0.lerp(p1, t);
    let b = p1.lerp(p2, t);
    let c = p2.lerp(p3, t);
    let d = a.lerp(&b, t);
    let e = b.lerp(&c, t);
    d.lerp(&e, t)
}
