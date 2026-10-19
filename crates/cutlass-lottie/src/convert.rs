//! Map a parsed [`crate::model::Lottie`] into a Cutlass [`Document`].
//!
//! The interesting work is translating Lottie's animated-property encoding into
//! Cutlass [`Track`]s with the correct temporal handles (`i`/`o`) and spatial
//! handles (`to`/`ti`), and turning keyframed `sh` paths into `Track<BezierPath>`
//! morphs. Unsupported features (trim paths, full gradients, text, mattes) are
//! approximated or skipped with a log warning rather than failing the import.

use crate::model::{Layer as LottieLayer, Lottie, Transform as LottieTransform};
use cutlass_anim::{Interp, Keyframe, SpatialTangents, Tangent, Time, Track};
use cutlass_core::scene::{Layer, LayerContent};
use cutlass_core::{Document, Scene, Transform2D};
use cutlass_shapes::{BezierPath, Paint, PathVertex, Shape, ShapeElement, Stroke};
use cutlass_time::Fps;
use glam::{Vec2, Vec4};
use serde_json::Value;
use std::f32::consts::PI;

/// Convert a whole composition into a document.
pub fn document(l: &Lottie) -> Document {
    let fps = parse_fps(l.fr);
    let mut scene = Scene::new(l.w, l.h, fps);
    scene.duration = Time::from_frame(l.op.max(0.0) as i64, fps);

    // Lottie paints earlier layers on top; Cutlass roots paint index 0 first
    // (backmost). Insert in reverse to preserve stacking order.
    for ll in l.layers.iter().rev() {
        scene.insert_root(convert_layer(ll, fps));
    }

    let title = if l.nm.is_empty() { "Lottie Import" } else { &l.nm };
    Document::new(title, scene)
}

fn parse_fps(fr: f64) -> Fps {
    if fr <= 0.0 {
        return Fps::from_int(30);
    }
    if (fr - fr.round()).abs() < 1e-6 {
        Fps::from_int(fr.round() as u32)
    } else {
        // Preserve fractional rates (e.g. 29.97) exactly enough to round-trip.
        Fps::new((fr * 1000.0).round() as u32, 1000)
    }
}

fn convert_layer(ll: &LottieLayer, fps: Fps) -> Layer {
    let transform = ll
        .ks
        .as_ref()
        .map(|ks| convert_transform(ks, fps))
        .unwrap_or_default();
    let start = Time::from_frame(ll.ip as i64, fps);
    let duration = Time::from_frame((ll.op - ll.ip).max(0.0) as i64, fps);
    let name = if ll.nm.is_empty() {
        format!("Layer {}", ll.ty)
    } else {
        ll.nm.clone()
    };

    // ty 4 = shape layer. Everything else becomes a transform-carrying group
    // placeholder for now (precomp/image/text/solid import comes later).
    if ll.ty == 4 {
        let mut elements = Vec::new();
        walk_shapes(&ll.shapes, fps, &PaintCtx::default(), &mut elements);
        match elements.len() {
            1 => {
                let mut layer = Layer::new(
                    name,
                    LayerContent::Shape(Box::new(elements.pop().unwrap())),
                    duration,
                )
                .at(start);
                layer.transform = transform;
                layer
            }
            _ => {
                let mut group = Layer::new(name, LayerContent::Group, duration).at(start);
                group.transform = transform;
                for (i, el) in elements.into_iter().enumerate() {
                    group.children.push(Layer::new(
                        format!("Shape {i}"),
                        LayerContent::Shape(Box::new(el)),
                        duration,
                    ));
                }
                group
            }
        }
    } else {
        if !matches!(ll.ty, 0 | 1 | 2 | 3 | 5) {
            log::warn!("lottie: unknown layer type {}", ll.ty);
        } else {
            log::warn!(
                "lottie: layer type {} not yet imported ('{}'); using a group placeholder",
                ll.ty,
                name
            );
        }
        let mut layer = Layer::new(name, LayerContent::Group, duration).at(start);
        layer.transform = transform;
        layer
    }
}

fn convert_transform(ks: &LottieTransform, fps: Fps) -> Transform2D {
    let mut t = Transform2D::default();
    if let Some(p) = &ks.p {
        t.position = build_vec2_track(p, fps, true, 1.0);
    }
    if let Some(a) = &ks.a {
        t.anchor = build_vec2_track(a, fps, false, 1.0);
    }
    if let Some(s) = &ks.s {
        t.scale = build_vec2_track(s, fps, false, 0.01);
    }
    if let Some(r) = &ks.r {
        t.rotation = build_track(r, fps, |v| num(v) * PI / 180.0);
    }
    if let Some(o) = &ks.o {
        t.opacity = build_track(o, fps, |v| num(v) * 0.01);
    }
    t
}

// --- shape tree -------------------------------------------------------------

#[derive(Default, Clone)]
struct PaintCtx {
    fill: Option<Paint>,
    stroke: Option<Stroke>,
}

/// Walk a Lottie shape item list, emitting a [`ShapeElement`] per geometry item.
/// Paints found in a group apply to that group's geometry (an approximation of
/// Lottie's paint stacking that is correct for typical exports).
fn walk_shapes(items: &[Value], fps: Fps, ctx: &PaintCtx, out: &mut Vec<ShapeElement>) {
    let mut local = ctx.clone();

    // First pass: gather paints declared in this list.
    for item in items {
        match item.get("ty").and_then(Value::as_str) {
            Some("fl") => local.fill = Some(Paint::Solid(color_track(&item["c"], fps))),
            Some("st") => {
                local.stroke = Some(Stroke {
                    paint: Paint::Solid(color_track(&item["c"], fps)),
                    width: build_track(&item["w"], fps, num),
                })
            }
            Some("gf") => local.fill = Some(Paint::Solid(Track::constant(first_gradient_color(item)))),
            Some("gs") => {
                local.stroke = Some(Stroke {
                    paint: Paint::Solid(Track::constant(first_gradient_color(item))),
                    width: build_track(&item["w"], fps, num),
                })
            }
            Some("tm") => log::warn!("lottie: trim paths (tm) not yet supported; skipping"),
            _ => {}
        }
    }

    // Second pass: emit geometry / recurse into groups.
    for item in items {
        match item.get("ty").and_then(Value::as_str) {
            Some("sh") => out.push(ShapeElement {
                shape: Shape::Path {
                    path: build_track(&item["ks"], fps, parse_path),
                },
                fill: local.fill.clone(),
                stroke: local.stroke.clone(),
            }),
            Some("rc") => out.push(ShapeElement {
                shape: Shape::Rect {
                    size: build_vec2_track(&item["s"], fps, false, 1.0),
                    corner_radius: build_track(&item["r"], fps, num),
                },
                fill: local.fill.clone(),
                stroke: local.stroke.clone(),
            }),
            Some("el") => out.push(ShapeElement {
                shape: Shape::Ellipse {
                    // Lottie size is full width/height; radii are half.
                    radii: build_vec2_track(&item["s"], fps, false, 0.5),
                },
                fill: local.fill.clone(),
                stroke: local.stroke.clone(),
            }),
            Some("gr") => {
                if let Some(children) = item.get("it").and_then(Value::as_array) {
                    walk_shapes(children, fps, &local, out);
                }
            }
            _ => {}
        }
    }
}

fn parse_path(v: &Value) -> BezierPath {
    // A keyframe value wraps the path object in a one-element array (`s: [..]`);
    // a static value is the object directly.
    let null = Value::Null;
    let obj = if v.is_array() {
        v.get(0).unwrap_or(&null)
    } else {
        v
    };
    let verts = obj.get("v").and_then(Value::as_array);
    let ins = obj.get("i").and_then(Value::as_array);
    let outs = obj.get("o").and_then(Value::as_array);
    let closed = obj.get("c").and_then(Value::as_bool).unwrap_or(false);

    let vertices = match verts {
        Some(verts) => verts
            .iter()
            .enumerate()
            .map(|(k, vert)| PathVertex {
                point: vec2(vert),
                in_tangent: ins.and_then(|a| a.get(k)).map(vec2).unwrap_or(Vec2::ZERO),
                out_tangent: outs.and_then(|a| a.get(k)).map(vec2).unwrap_or(Vec2::ZERO),
            })
            .collect(),
        None => Vec::new(),
    };
    BezierPath { closed, vertices }
}

// --- property / keyframe parsing -------------------------------------------

/// Build a [`Track`] from a Lottie animated-property object, parsing each value
/// with `parse` and translating temporal handles. `i`/`o` in Lottie both live on
/// the *start* keyframe of a segment; Cutlass stores the in-handle on the *end*
/// keyframe, so we shift `i` forward by one.
fn build_track<T, F>(prop: &Value, fps: Fps, parse: F) -> Track<T>
where
    T: Clone,
    F: Fn(&Value) -> T,
{
    let animated = prop.get("a").and_then(Value::as_i64) == Some(1);
    let k = &prop["k"];
    if !animated {
        return Track::constant(parse(k));
    }
    let kfs = match k.as_array() {
        Some(a) if !a.is_empty() => a,
        _ => return Track::constant(parse(k)),
    };

    let mut keys: Vec<Keyframe<T>> = kfs
        .iter()
        .map(|kf| Keyframe::new(frame_time(&kf["t"], fps), parse(&kf["s"])))
        .collect();

    // Lottie stores both handles of a segment on its start keyframe; shift the
    // in-handle (`i`) onto the next key to match Cutlass's per-key handles.
    for (n, kf) in kfs.iter().enumerate().take(kfs.len() - 1) {
        if kf.get("h").and_then(Value::as_i64) == Some(1) {
            keys[n].interp = Interp::Hold;
        } else if kf.get("o").is_some() && kf.get("i").is_some() {
            keys[n].interp = Interp::Bezier;
            keys[n].out_tangent = tangent(&kf["o"]);
            keys[n + 1].in_tangent = tangent(&kf["i"]);
        }
    }

    let default = keys[0].value.clone();
    Track { default, keys }
}

/// Like [`build_track`] for `Vec2`, optionally attaching spatial motion-path
/// handles (`to`/`ti`).
fn build_vec2_track(prop: &Value, fps: Fps, spatial: bool, scale: f32) -> Track<Vec2> {
    let mut track = build_track(prop, fps, |v| vec2(v) * scale);

    if spatial && prop.get("a").and_then(Value::as_i64) == Some(1) {
        if let Some(kfs) = prop["k"].as_array() {
            let has_spatial = kfs.iter().any(|kf| kf.get("to").is_some());
            if has_spatial && track.keys.len() == kfs.len() {
                for key in track.keys.iter_mut() {
                    key.spatial = Some(Box::new(SpatialTangents {
                        out: Vec2::ZERO,
                        in_: Vec2::ZERO,
                    }));
                }
                for (n, kf) in kfs.iter().enumerate().take(kfs.len() - 1) {
                    let to = kf.get("to").map(vec2).unwrap_or(Vec2::ZERO);
                    let ti = kf.get("ti").map(vec2).unwrap_or(Vec2::ZERO);
                    if let Some(s) = track.keys[n].spatial.as_mut() {
                        s.out = to;
                    }
                    if let Some(s) = track.keys[n + 1].spatial.as_mut() {
                        s.in_ = ti;
                    }
                }
            }
        }
    }
    track
}

fn color_track(prop: &Value, fps: Fps) -> Track<Vec4> {
    build_track(prop, fps, |v| {
        let a = if v.as_array().map(|a| a.len()).unwrap_or(0) >= 4 {
            comp(v, 3)
        } else {
            1.0
        };
        Vec4::new(comp(v, 0), comp(v, 1), comp(v, 2), a)
    })
}

/// Pull the first color stop out of a gradient fill/stroke (`g.k.k` is a flat
/// `[pos, r, g, b, ...]` array). A full gradient model comes later.
fn first_gradient_color(item: &Value) -> Vec4 {
    if let Some(stops) = item
        .get("g")
        .and_then(|g| g.get("k"))
        .and_then(|k| k.get("k"))
        .and_then(Value::as_array)
    {
        if stops.len() >= 4 {
            return Vec4::new(
                num(&stops[1]),
                num(&stops[2]),
                num(&stops[3]),
                1.0,
            );
        }
    }
    Vec4::ONE
}

// --- scalar / vector readers ------------------------------------------------

/// Read a scalar from a number, or the first element of an array.
fn num(v: &Value) -> f32 {
    match v {
        Value::Number(n) => n.as_f64().unwrap_or(0.0) as f32,
        Value::Array(a) => a.first().map(num).unwrap_or(0.0),
        _ => 0.0,
    }
}

/// Read the `i`-th element of an array value as a scalar (0 if absent).
fn comp(v: &Value, i: usize) -> f32 {
    v.get(i).map(num).unwrap_or(0.0)
}

fn vec2(v: &Value) -> Vec2 {
    Vec2::new(comp(v, 0), comp(v, 1))
}

fn tangent(v: &Value) -> Tangent {
    Tangent::new(num(&v["x"]), num(&v["y"]))
}

fn frame_time(v: &Value, fps: Fps) -> Time {
    Time::from_frame(num(v).round() as i64, fps)
}
