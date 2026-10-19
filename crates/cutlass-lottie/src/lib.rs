//! # cutlass-lottie
//!
//! Import [Lottie](https://lottiefiles.com/) / Bodymovin animations into the
//! Cutlass [`Document`] model. This is the proof that the engine's animation
//! model is rich enough for real motion design: Lottie's per-keyframe temporal
//! handles (`i`/`o`), spatial motion paths (`to`/`ti`), and **keyframed bezier
//! paths** map onto Cutlass tracks, transforms, and `Track<BezierPath>` morphs.
//!
//! ## Scope
//!
//! Implemented: composition setup (size/fps/duration), layer transforms
//! (position with motion paths, anchor, scale, rotation, opacity) with full
//! temporal easing, and shape layers (paths — static and morphing — plus
//! rect/ellipse with solid fills/strokes).
//!
//! Approximated / TODO (logged, never fatal): gradients (first stop only), trim
//! paths, mattes/masks, precomp/image/text/solid layers, and per-dimension
//! temporal easing. The importer degrades gracefully so partial files still load.
//!
//! ```no_run
//! let doc = cutlass_lottie::import_file("animation.json").unwrap();
//! println!("{} layers", doc.scene.len());
//! ```

mod convert;
mod model;

use cutlass_core::Document;
use std::path::Path;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum LottieError {
    #[error("failed to parse Lottie JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

/// Import a Lottie animation from a JSON string.
pub fn import_str(json: &str) -> Result<Document, LottieError> {
    let lottie: model::Lottie = serde_json::from_str(json)?;
    Ok(convert::document(&lottie))
}

/// Import a Lottie animation from raw JSON bytes.
pub fn import_slice(bytes: &[u8]) -> Result<Document, LottieError> {
    let lottie: model::Lottie = serde_json::from_slice(bytes)?;
    Ok(convert::document(&lottie))
}

/// Import a Lottie animation from a `.json` file on disk.
pub fn import_file(path: impl AsRef<Path>) -> Result<Document, LottieError> {
    let json = std::fs::read_to_string(path)?;
    import_str(&json)
}

#[cfg(test)]
mod tests {
    use super::*;
    use cutlass_anim::Sample;
    use cutlass_core::scene::LayerContent;
    use cutlass_shapes::Shape;
    use cutlass_time::Time;

    const WELCOME: &str = include_str!("../../../test-assets/Welcome.json");

    #[test]
    fn imports_welcome_composition_metadata() {
        let doc = import_str(WELCOME).unwrap();
        assert_eq!(doc.scene.width, 428);
        assert_eq!(doc.scene.height, 123);
        assert_eq!(doc.scene.fps.as_f64(), 60.0);
        // op = 493 frames @ 60fps = ~8.217s.
        assert!((doc.scene.duration.as_seconds() - 493.0 / 60.0).abs() < 1e-3);
        assert_eq!(doc.scene.roots().len(), 1);
    }

    #[test]
    fn imports_animated_bezier_path_with_temporal_tangents() {
        let doc = import_str(WELCOME).unwrap();
        let root = doc.scene.root_nodes().next().expect("one layer");

        // The single shape layer carries the morphing path (1 shape => Shape).
        let shape = match &root.content {
            LayerContent::Shape(s) => s,
            other => panic!("expected a shape layer, got {other:?}"),
        };
        let Shape::Path { path } = &shape.shape else {
            panic!("expected a path shape");
        };

        // 4 path keyframes in Welcome.json (t = 12, 110, 196, 292).
        assert_eq!(path.keys.len(), 4, "animated path should have 4 keyframes");
        assert!(path.is_animated());

        // First segment uses bezier easing (i/o present on the first keyframe).
        assert_eq!(path.keys[0].interp, cutlass_anim::Interp::Bezier);
        // The in-handle of segment 0 (Lottie i:{0.315,1}) was shifted onto key 1.
        assert!((path.keys[1].in_tangent.x - 0.315).abs() < 1e-3);
        assert!((path.keys[0].out_tangent.x - 0.333).abs() < 1e-3);

        // Each path keyframe is the 32-vertex glyph outline.
        assert_eq!(path.keys[0].value.vertices.len(), 32);

        // Sampling between keyframes yields a morphed path of the same arity.
        let mid = path.sample(Time::from_frame(60, doc.scene.fps));
        assert_eq!(mid.vertices.len(), 32);
        // The first vertex x morphs between -188.452 (t=12) and -187.452 (t=110).
        let x = mid.vertices[0].point.x;
        assert!(x > -188.5 && x < -187.0, "morphed x out of range: {x}");
    }

    #[test]
    fn layer_transform_is_imported() {
        let doc = import_str(WELCOME).unwrap();
        let root = doc.scene.root_nodes().next().unwrap();
        // Static position [210.962, 63.445].
        let pos = root.transform.position.sample(Time::ZERO);
        assert!((pos.x - 210.962).abs() < 1e-2);
        assert!((pos.y - 63.445).abs() < 1e-2);
        // Opacity 100 => 1.0 after the 0.01 scale.
        assert!((root.transform.opacity.sample(Time::ZERO) - 1.0).abs() < 1e-4);
    }
}
