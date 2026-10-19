//! # cutlass-shapes
//!
//! Vector shapes live in the same scene graph and are driven by the same
//! animation engine as everything else, so a rectangle's corner radius or a
//! path's control points animate exactly like a transform or a shader uniform.
//!
//! Rendering is GPU path tessellation (vello/lyon) wired in later; this crate
//! defines the animatable geometry + paint model.

use cutlass_anim::{Lerp, Track};
use glam::{Vec2, Vec4};
use serde::{Deserialize, Serialize};

/// A fill or stroke paint. Color is animatable; gradients/patterns extend this.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Paint {
    /// Solid RGBA in linear space.
    Solid(Track<Vec4>),
    /// Two-stop linear gradient; full gradient stops come later.
    LinearGradient {
        from: Vec2,
        to: Vec2,
        start: Track<Vec4>,
        end: Track<Vec4>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Stroke {
    pub paint: Paint,
    pub width: Track<f32>,
}

/// A single, flattened path command for the rasterizer. Produced from a
/// [`BezierPath`] via [`BezierPath::to_path_cmds`].
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum PathCmd {
    MoveTo(Vec2),
    LineTo(Vec2),
    CubicTo(Vec2, Vec2, Vec2),
    Close,
}

/// One control vertex of a [`BezierPath`]. Tangents are stored **relative to the
/// point** (After Effects / Lottie convention), which is what makes path morphs
/// interpolate cleanly: corresponding vertices and their handles blend pairwise.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct PathVertex {
    pub point: Vec2,
    /// Incoming handle, relative to `point` (Lottie `i`).
    #[serde(default)]
    pub in_tangent: Vec2,
    /// Outgoing handle, relative to `point` (Lottie `o`).
    #[serde(default)]
    pub out_tangent: Vec2,
}

/// An animatable cubic-Bézier path: the geometry itself can be keyframed
/// (`Track<BezierPath>`), so shapes morph — the headline capability needed to
/// represent real Lottie/After Effects shape animation.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct BezierPath {
    #[serde(default)]
    pub closed: bool,
    pub vertices: Vec<PathVertex>,
}

impl BezierPath {
    /// Flatten to draw commands. Each segment becomes a cubic using the
    /// neighboring vertices' (absolute) tangent handles.
    pub fn to_path_cmds(&self) -> Vec<PathCmd> {
        let mut cmds = Vec::new();
        let Some(first) = self.vertices.first() else {
            return cmds;
        };
        cmds.push(PathCmd::MoveTo(first.point));
        for pair in self.vertices.windows(2) {
            let (a, b) = (&pair[0], &pair[1]);
            cmds.push(PathCmd::CubicTo(
                a.point + a.out_tangent,
                b.point + b.in_tangent,
                b.point,
            ));
        }
        if self.closed && self.vertices.len() > 1 {
            let a = self.vertices.last().unwrap();
            cmds.push(PathCmd::CubicTo(
                a.point + a.out_tangent,
                first.point + first.in_tangent,
                first.point,
            ));
            cmds.push(PathCmd::Close);
        }
        cmds
    }
}

impl Lerp for BezierPath {
    /// Morph between two paths by blending corresponding vertices. Vertex counts
    /// should match (After Effects enforces this for shape interpolation); if
    /// they differ we blend the common prefix.
    fn lerp(&self, other: &Self, t: f32) -> Self {
        let n = self.vertices.len().min(other.vertices.len());
        let vertices = (0..n)
            .map(|k| {
                let a = &self.vertices[k];
                let b = &other.vertices[k];
                PathVertex {
                    point: a.point.lerp(b.point, t),
                    in_tangent: a.in_tangent.lerp(b.in_tangent, t),
                    out_tangent: a.out_tangent.lerp(b.out_tangent, t),
                }
            })
            .collect();
        BezierPath {
            closed: self.closed,
            vertices,
        }
    }
}

/// Built-in primitives plus arbitrary paths. Primitives carry animatable
/// parameters; freeform (and morphing) geometry uses [`Shape::Path`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Shape {
    Rect {
        size: Track<Vec2>,
        corner_radius: Track<f32>,
    },
    Ellipse {
        radii: Track<Vec2>,
    },
    /// Freeform geometry whose vertices can animate (path morph).
    Path {
        path: Track<BezierPath>,
    },
}

/// A drawable shape element: geometry plus paint.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ShapeElement {
    pub shape: Shape,
    pub fill: Option<Paint>,
    pub stroke: Option<Stroke>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use cutlass_anim::{Keyframe, Sample, Time};

    fn tri(scale: f32) -> BezierPath {
        BezierPath {
            closed: true,
            vertices: vec![
                PathVertex {
                    point: Vec2::new(0.0, 0.0),
                    ..Default::default()
                },
                PathVertex {
                    point: Vec2::new(scale, 0.0),
                    ..Default::default()
                },
                PathVertex {
                    point: Vec2::new(0.0, scale),
                    ..Default::default()
                },
            ],
        }
    }

    #[test]
    fn to_path_cmds_emits_move_cubics_and_close() {
        let cmds = tri(10.0).to_path_cmds();
        assert!(matches!(cmds.first(), Some(PathCmd::MoveTo(_))));
        assert!(matches!(cmds.last(), Some(PathCmd::Close)));
        // 3 vertices closed => MoveTo + 2 inner cubics + 1 closing cubic + Close.
        assert_eq!(cmds.len(), 5);
    }

    #[test]
    fn path_morph_interpolates_vertices() {
        let mut track = Track::constant(tri(10.0));
        track.insert(Keyframe::new(Time::from_seconds(0.0), tri(10.0)));
        track.insert(Keyframe::new(Time::from_seconds(1.0), tri(20.0)));
        let mid = track.sample(Time::from_seconds(0.5));
        // The second vertex x should be halfway between 10 and 20.
        assert!((mid.vertices[1].point.x - 15.0).abs() < 1e-4);
        assert_eq!(mid.vertices.len(), 3);
    }
}
