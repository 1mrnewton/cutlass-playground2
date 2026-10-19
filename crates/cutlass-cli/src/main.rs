//! Headless dev harness for the Cutlass engine.
//!
//! Builds a small animated scene from the data model, exercises the AI agent
//! (which emits atomic commands the engine runs), and demonstrates that undo is
//! free now that every command returns its inverse.

use cutlass_agent::{AddCaptions, Agent, AgentContext, CutSilences};
use cutlass_anim::{Keyframe, Tangent, Time, Track};
use cutlass_core::scene::{Layer, LayerContent, Scene, SceneId};
use cutlass_core::{
    AudioClip, Command, Document, Editor, FadeCurve, Fps, SceneSnapshot, SharedScene, TimeWarp,
    TrackKind, Transform2D,
};
use cutlass_ml::{Caption, MlError, Models, SilenceDetector, Span, SpeechToText, Transcript};
use cutlass_render::{compositing_clip_ids, RenderMode};
use cutlass_shapes::{Paint, Shape, ShapeElement};
use cutlass_text::{Align, FontRef, TextBlock, TextRun, TextStyle};
use glam::{Vec2, Vec4};

/// A canned speech-to-text provider so the agent demo runs without a real model.
struct StubStt;
impl SpeechToText for StubStt {
    fn transcribe(&self, _source: &str, _language: Option<&str>) -> Result<Transcript, MlError> {
        Ok(Transcript {
            language: "en".into(),
            captions: vec![
                Caption {
                    span: Span { start: 0.0, end: 1.5 },
                    text: "Welcome to Cutlass".into(),
                },
                Caption {
                    span: Span { start: 1.8, end: 3.2 },
                    text: "the open-source AI editor".into(),
                },
            ],
        })
    }
}

/// A canned silence detector (one silent gap in the middle of the clip).
struct StubSilence;
impl SilenceDetector for StubSilence {
    fn detect(&self, _source: &str, _threshold_db: f32, _min_len: f64) -> Result<Vec<Span>, MlError> {
        Ok(vec![Span { start: 1.5, end: 1.8 }])
    }
}

fn sample_document() -> Document {
    let mut scene = Scene::new(1920, 1080, Fps::from_int(30));
    scene.duration = Time::from_seconds(5.0);

    let five = Time::from_seconds(5.0);

    // A media clip the AI agent can analyze (transcribe / detect silences).
    // Speed-ramped: starts at 0.5× (slow-mo) and ramps to 2× by the end.
    let mut speed = Track::constant(1.0);
    speed.insert(Keyframe::new(Time::ZERO, 0.5));
    speed.insert(Keyframe::new(five, 2.0));
    let clip = Layer::new(
        "Clip",
        LayerContent::Media {
            source: "clip-1".into(),
            in_point: Time::ZERO,
            out_point: five,
            warp: Some(TimeWarp::Ramp(speed)),
        },
        five,
    );

    // A background rectangle.
    let bg = Layer::new(
        "Background",
        LayerContent::Shape(Box::new(ShapeElement {
            shape: Shape::Rect {
                size: Track::constant(Vec2::new(1920.0, 1080.0)),
                corner_radius: Track::constant(0.0),
            },
            fill: Some(Paint::Solid(Track::constant(Vec4::new(0.05, 0.05, 0.08, 1.0)))),
            stroke: None,
        })),
        five,
    );

    // An animated title that fades + slides in.
    let mut title = Layer::new(
        "Title",
        LayerContent::Text(Box::new(TextBlock {
            runs: vec![TextRun {
                text: "Cutlass".into(),
                style: TextStyle {
                    font: FontRef {
                        family: "Inter".into(),
                        weight: 700,
                        italic: false,
                    },
                    align: Align::Center,
                    fill: Track::constant(Vec4::ONE),
                    size: Track::constant(120.0),
                    tracking: Track::constant(0.0),
                    line_height: 1.2,
                },
            }],
            width: 1200.0,
            height: None,
        })),
        five,
    );

    // Fade + slide in, eased with per-keyframe tangents (the segment uses the
    // first key's `out` handle and the second key's `in` handle).
    let mut opacity = Track::constant(0.0);
    opacity.insert(Keyframe::new(Time::from_seconds(0.0), 0.0).with_out_tangent(Tangent::new(0.0, 0.0)));
    opacity.insert(Keyframe::new(Time::from_seconds(0.8), 1.0).with_in_tangent(Tangent::new(0.58, 1.0)));
    let mut position = Track::constant(Vec2::new(0.0, 40.0));
    position.insert(
        Keyframe::new(Time::from_seconds(0.0), Vec2::new(0.0, 40.0))
            .with_out_tangent(Tangent::new(0.2, 0.0)),
    );
    position.insert(Keyframe::new(Time::from_seconds(0.8), Vec2::ZERO).with_in_tangent(Tangent::new(0.0, 1.0)));
    title.transform = Transform2D {
        opacity,
        position,
        ..Default::default()
    };

    // A music bed: -6 dB, equal-power fade in/out — first-class audio on the
    // timeline with keyframeable gain/pan and fades.
    let mut music = AudioClip::new("music-1", Time::ZERO, five);
    music.params.gain_db = Track::constant(-6.0);
    music.params.fade_in = Time::from_seconds(0.5);
    music.params.fade_out = Time::from_seconds(1.0);
    music.params.fade_curve = FadeCurve::EqualPower;
    let music_layer = Layer::new("Music", LayerContent::Audio(Box::new(music)), five);

    scene.insert_root(clip);
    scene.insert_root(bg);
    scene.insert_root(title);
    scene.insert_root(music_layer);

    let mut doc = Document::new("Demo Project", scene);
    doc.fonts.push(("Inter".into(), "fonts/Inter-Bold.ttf".into()));
    doc.sources.push(("clip-1".into(), "media/clip1.mp4".into()));
    doc.sources.push(("music-1".into(), "media/music.mp3".into()));
    doc
}

fn main() {
    let doc = sample_document();

    // `cutlass emit <path>`: write the sample document as a portable template.
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some("emit") {
        let path = args.get(2).map(String::as_str).unwrap_or("demo.cutlass.json");
        match doc.to_json() {
            Ok(json) => {
                if let Err(e) = std::fs::write(path, json) {
                    eprintln!("write error: {e}");
                    std::process::exit(1);
                }
                println!("wrote {path}");
            }
            Err(e) => {
                eprintln!("serialize error: {e}");
                std::process::exit(1);
            }
        }
        return;
    }

    // `cutlass import <lottie.json>`: load a Lottie animation and summarize it.
    if args.get(1).map(String::as_str) == Some("import") {
        let Some(path) = args.get(2) else {
            eprintln!("usage: cutlass import <lottie.json>");
            std::process::exit(2);
        };
        match cutlass_lottie::import_file(path) {
            Ok(imported) => describe_import(&imported),
            Err(e) => {
                eprintln!("import error: {e}");
                std::process::exit(1);
            }
        }
        return;
    }

    println!("Cutlass dev harness");
    println!("  schema version : {}", doc.schema_version);
    println!(
        "  scene          : {}x{} @ {} fps, {:.1}s",
        doc.scene.width,
        doc.scene.height,
        doc.scene.fps.as_f64(),
        doc.scene.duration.as_seconds()
    );
    println!("  nodes          : {}", doc.scene.len());

    // Render-side code only ever sees an immutable snapshot (never `doc.scene`).
    let snapshot = SceneSnapshot::new(doc.scene.clone());
    for t in [0.0_f64, 0.4, 0.8, 2.0] {
        let paint = compositing_clip_ids(&snapshot, Time::from_seconds(t));
        println!("  t={t:>4.1}s  clips to paint={}", paint.len());
    }

    // Audio mix readout: the mixer asks the snapshot for active clips and the
    // pure mix math for their exact gain (same numbers preview and export use).
    let t = Time::from_seconds(2.0);
    for id in snapshot.active_audio_at(t) {
        if let Some(node) = snapshot.get(id) {
            if let LayerContent::Audio(clip) = &node.content {
                let (l, r) = clip.params.stereo_gain_at(t, node.range);
                println!("  audio '{}' L/R  : {l:.3} / {r:.3} @ {}s", node.name, t.as_seconds());
            }
        }
    }

    // Time-remap readout: which source time the decoder reads as the speed-ramped
    // clip plays (clip-local timeline time -> source time).
    if let Some(node) = snapshot
        .scene()
        .root_nodes()
        .find(|n| matches!(n.content, LayerContent::Media { .. }))
    {
        for local in [0.0_f64, 2.5, 5.0] {
            let lt = Time::from_seconds(local);
            if let (Some(src), Some(speed)) =
                (node.content.source_time_at(lt), node.content.speed_at(lt))
            {
                println!(
                    "  remap '{}'   : local {local:.1}s -> source {:.2}s (speed {speed:.2}x)",
                    node.name,
                    src.as_seconds()
                );
            }
        }
    }

    let mode = RenderMode::Export {
        fps: Fps::from_int(30),
    };
    println!("  render mode    : {mode:?}");

    match doc.to_json() {
        Ok(json) => {
            println!("  serialized     : {} bytes of portable JSON", json.len());
            // Load it back through the version-aware migrator (C4): save → load.
            match Document::from_json(&json) {
                Ok(back) => println!(
                    "  loaded back    : schema v{}, '{}' ({} nodes)",
                    back.schema_version,
                    back.title,
                    back.scene.len()
                ),
                Err(e) => eprintln!("  load error     : {e}"),
            }
        }
        Err(e) => eprintln!("  serialize error: {e}"),
    }

    agent_demo(doc);
    threading_demo();
    tracks_demo();
    color_demo();
}

/// Demonstrates H7: one linear working space, with encode/decode only at the I/O
/// boundary — the same color picked once, delivered to SDR and HDR.
fn color_demo() {
    use cutlass_core::{ColorConfig, ColorSpace, LinearRgba};

    println!("\nColor management (H7: linear working space + encode/decode boundary)");

    // A swatch picked in sRGB is decoded into the linear working space, where all
    // compositing/effects math happens.
    let orange = LinearRgba::from_srgb8(255, 128, 0, 255);
    println!(
        "  pick sRGB #FF8000 -> linear working ({:.3}, {:.3}, {:.3})",
        orange.r, orange.g, orange.b
    );

    // SDR delivery: encode straight back to sRGB.
    let sdr = ColorConfig::default();
    let s = sdr.encode_output(orange);
    println!(
        "  -> SDR sRGB out   : ({:.3}, {:.3}, {:.3})   hdr={}",
        s.x, s.y, s.z, sdr.is_hdr()
    );

    // HDR delivery: the *same* working color (same linear working space)
    // mastered to Rec.2020 + PQ — only the output transform differs.
    let hdr = ColorConfig {
        output: ColorSpace::REC2020_PQ,
        ..ColorConfig::default()
    };
    let p = hdr.encode_output(orange);
    println!(
        "  -> Rec2020/PQ out : ({:.3}, {:.3}, {:.3})   hdr={}",
        p.x, p.y, p.z, hdr.is_hdr()
    );
}

/// Demonstrates H5: an explicit multi-track timeline with compositing order, and
/// mute/solo as undoable commands that change what the renderer/mixer see.
fn tracks_demo() {
    println!("\nTracks / lanes (H5: explicit timeline, compositing order, mute/solo)");

    let five = Time::from_seconds(5.0);
    let mut scene = Scene::new(1920, 1080, Fps::from_int(30));
    scene.duration = five;

    // A realistic stack, back (index 0) to front: BG → Main video → Titles → Music.
    let bg_tk = scene.add_track("BG", TrackKind::Video);
    scene
        .insert(Some(bg_tk), None, Layer::new("bg", LayerContent::Group, five))
        .unwrap();
    let main_tk = scene.add_track("Main", TrackKind::Video);
    scene
        .insert(
            Some(main_tk),
            None,
            Layer::new(
                "clip",
                LayerContent::Media {
                    source: "v".into(),
                    in_point: Time::ZERO,
                    out_point: five,
                    warp: None,
                },
                five,
            ),
        )
        .unwrap();
    let title_tk = scene.add_track("Titles", TrackKind::Subtitle);
    scene
        .insert(
            Some(title_tk),
            None,
            Layer::new("title", LayerContent::Group, five),
        )
        .unwrap();
    let music_tk = scene.add_track("Music", TrackKind::Audio);
    scene
        .insert(
            Some(music_tk),
            None,
            Layer::new(
                "music",
                LayerContent::Audio(Box::new(AudioClip::new("m", Time::ZERO, five))),
                five,
            ),
        )
        .unwrap();

    let mut ed = Editor::new(Document::new("tracks", scene));
    let t = Time::from_seconds(2.0);
    let names = |ed: &Editor, ids: &[SceneId]| -> Vec<String> {
        ids.iter()
            .filter_map(|&id| ed.document.scene.get(id).map(|n| n.name.clone()))
            .collect()
    };

    println!(
        "  tracks         : {:?}",
        ed.document
            .scene
            .tracks()
            .map(|n| n.name.clone())
            .collect::<Vec<_>>()
    );
    println!(
        "  paint order    : {:?}",
        names(&ed, &ed.document.scene.compositing_clips_at(t))
    );
    println!(
        "  audible        : {:?}",
        names(&ed, &ed.document.scene.active_audio_at(t))
    );

    // Hide the BG lane via an undoable command; the renderer's input changes.
    let mut hidden = ed.document.scene.get(bg_tk).unwrap().track_meta().unwrap().clone();
    hidden.enabled = false;
    ed.apply(Command::SetTrackMeta {
        id: bg_tk,
        meta: Box::new(hidden),
    })
    .unwrap();
    println!(
        "  hide BG        : {:?}",
        names(&ed, &ed.document.scene.compositing_clips_at(t))
    );
    ed.undo();
    println!(
        "  undo hide      : {:?}",
        names(&ed, &ed.document.scene.compositing_clips_at(t))
    );

    // Solo the Main lane: only it composites (others silenced).
    let mut solo = ed.document.scene.get(main_tk).unwrap().track_meta().unwrap().clone();
    solo.solo = true;
    ed.apply(Command::SetTrackMeta {
        id: main_tk,
        meta: Box::new(solo),
    })
    .unwrap();
    println!(
        "  solo Main      : {:?}",
        names(&ed, &ed.document.scene.compositing_clips_at(t))
    );
}

/// Demonstrates H1/H2: a render thread that *only* ever sees immutable
/// [`SceneSnapshot`]s, fed lock-free by the edit thread via [`SharedScene`].
///
/// The render thread can sample/composite at any time without touching — or
/// racing — the live, mutating document on the edit thread.
fn threading_demo() {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use std::thread;
    use std::time::Duration;

    println!("\nThreaded render handoff (H1/H2: immutable snapshot, lock-free)");

    let mut editor = Editor::new(sample_document());
    let shared = Arc::new(SharedScene::new(editor.snapshot()));
    let stop = Arc::new(AtomicBool::new(false));

    // Render thread: pulls the latest snapshot each "frame" and composites from
    // it. It never holds a reference into the edit thread's scene.
    let render_shared = Arc::clone(&shared);
    let render_stop = Arc::clone(&stop);
    let render = thread::spawn(move || {
        let mut frames = 0u32;
        let mut max_nodes = 0usize;
        while !render_stop.load(Ordering::Relaxed) {
            let snap = render_shared.latest();
            let active = compositing_clip_ids(&snap, Time::from_seconds(0.0)).len();
            max_nodes = max_nodes.max(snap.len());
            let _ = active; // a real renderer would composite these here
            frames += 1;
            thread::sleep(Duration::from_micros(200));
        }
        (frames, max_nodes)
    });

    // Edit thread: apply commands and publish a fresh snapshot after each.
    for i in 0..4 {
        let layer = Layer::new(
            format!("Live edit {i}"),
            LayerContent::Group,
            Time::from_seconds(1.0),
        );
        let _ = editor.apply(Command::AddLayer {
            parent: None,
            index: None,
            layer: Box::new(layer),
        });
        shared.publish(editor.snapshot());
        thread::sleep(Duration::from_millis(1));
    }

    stop.store(true, Ordering::Relaxed);
    let (frames, max_nodes) = render.join().unwrap();
    println!("  render thread frames     : {frames}");
    println!("  max nodes it ever saw    : {max_nodes}");
    println!("  final live scene nodes   : {}", editor.document.scene.len());
}

/// Demonstrates the AI flow + free undo: the agent emits one atomic `Command`
/// per intent, the engine runs it through the same `Editor` (and undo stack) as
/// manual edits, and undo reverses it via the command's inverse.
fn agent_demo(doc: Document) {
    println!("\nAI agent demo (atomic commands + inverse-based undo)");

    let models = Models {
        stt: Some(Box::new(StubStt)),
        silence: Some(Box::new(StubSilence)),
        ..Default::default()
    };

    let mut agent = Agent::new();
    agent
        .register(Box::new(AddCaptions::default()))
        .register(Box::new(CutSilences::default()));
    println!("  skills         : {:?}", agent.skill_names());

    let mut editor = Editor::new(doc);
    let start_nodes = editor.document.scene.len();

    for intent in ["add_captions", "cut_silences"] {
        let planned = {
            let ctx = AgentContext {
                document: &editor.document,
                models: &models,
            };
            agent.run(intent, &ctx)
        };
        match planned {
            Ok(cmd) => {
                println!("  '{intent}' -> {}", describe(&cmd));
                if let Command::Batch(children) = &cmd {
                    for ch in children {
                        println!("      {}", describe(ch));
                    }
                }
                match editor.apply(cmd) {
                    Ok(change) => println!("      change: {}", describe_change(&change)),
                    Err(e) => eprintln!("      apply error: {e}"),
                }
            }
            Err(e) => eprintln!("  '{intent}' failed: {e}"),
        }
    }

    println!(
        "  after edits    : {} nodes, undo depth {}",
        editor.document.scene.len(),
        editor.undo_depth()
    );

    // Each skill = exactly one undo step. Undo both, expect the original scene.
    editor.undo();
    editor.undo();
    println!(
        "  after 2x undo  : {} nodes (started with {})",
        editor.document.scene.len(),
        start_nodes
    );
}

/// Summarize an imported document, highlighting animated geometry/transforms —
/// the C5 capabilities (path morphs + tangents) that motivated the importer.
fn describe_import(doc: &Document) {
    use cutlass_core::scene::LayerContent;

    println!("Imported Lottie document");
    println!("  title          : {}", doc.title);
    println!(
        "  scene          : {}x{} @ {} fps, {:.2}s",
        doc.scene.width,
        doc.scene.height,
        doc.scene.fps.as_f64(),
        doc.scene.duration.as_seconds()
    );
    println!("  layers         : {}", doc.scene.len());

    for node in doc.scene.root_nodes() {
        let pos = {
            use cutlass_anim::Sample;
            node.transform.position.sample(Time::ZERO)
        };
        let anim = if node.transform.position.is_animated() {
            " (animated)"
        } else {
            ""
        };
        println!(
            "  - '{}' @ ({:.1}, {:.1}){}",
            node.name, pos.x, pos.y, anim
        );
        if let LayerContent::Shape(el) = &node.content {
            if let Shape::Path { path } = &el.shape {
                let verts = path.keys.first().map(|k| k.value.vertices.len()).unwrap_or(0);
                println!(
                    "      path: {} keyframe(s), {} vertices, morphing={}",
                    path.keys.len().max(1),
                    verts,
                    path.is_animated()
                );
            }
        }
    }
}

fn describe(cmd: &Command) -> String {
    match cmd {
        Command::AddLayer { layer, .. } => format!(
            "AddLayer({}, {:.2}s @ {:.2}s)",
            layer.name,
            layer.duration.as_seconds(),
            layer.start.as_seconds()
        ),
        Command::RemoveLayer { id } => format!("RemoveLayer({})", id.0),
        Command::Rename { name, .. } => format!("Rename({name})"),
        Command::SetTiming { start, duration, .. } => format!(
            "SetTiming({:.2}s, {:.2}s)",
            start.as_seconds(),
            duration.as_seconds()
        ),
        Command::Reparent { .. } => "Reparent".to_string(),
        Command::SetTrackMeta { meta, .. } => format!("SetTrackMeta({:?})", meta.kind),
        Command::Batch(cmds) => format!("Batch[{} commands]", cmds.len()),
    }
}

/// Render the granular change footprint (H6) the UI would receive as an event.
fn describe_change(c: &cutlass_core::ChangeSet) -> String {
    let span = c
        .time
        .map(|r| format!("{:.2}s..{:.2}s", r.start.as_seconds(), r.end().as_seconds()))
        .unwrap_or_else(|| "—".into());
    format!(
        "{} layer(s), span {span}, structural={}",
        c.layers.len(),
        c.structural
    )
}
