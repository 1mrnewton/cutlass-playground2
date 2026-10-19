//! Built-in agent skills. Each is a pure `context -> Command` planner returning
//! one atomic command (a `Batch` when multi-step), so every skill invocation is
//! exactly one undo step and flows through the normal editor.
//!
//! These are intentionally lightweight placeholders that demonstrate the data
//! flow (analyze via `cutlass-ml` → emit `cutlass-core` commands). Real tuning,
//! source-trim adjustment, and an LLM planner come later.

use crate::{AgentContext, AgentError, Skill};
use cutlass_anim::{Time, Track};
use cutlass_core::scene::{Layer, LayerContent, Node, SceneId};
use cutlass_core::Command;
use cutlass_ml::MlError;
use cutlass_text::{Align, FontRef, TextBlock, TextRun, TextStyle};
use glam::Vec4;

/// "Add captions": transcribe the first media source and add a styled,
/// time-coded text layer per caption segment, as one atomic edit.
#[derive(Default)]
pub struct AddCaptions {
    pub language: Option<String>,
}

impl Skill for AddCaptions {
    fn name(&self) -> &'static str {
        "add_captions"
    }

    fn plan(&self, ctx: &AgentContext<'_>) -> Result<Command, AgentError> {
        let stt = ctx
            .models
            .stt
            .as_ref()
            .ok_or(MlError::NoProvider("speech_to_text"))?;

        let source = first_source(ctx).ok_or_else(|| AgentError::Skill("no media source".into()))?;
        let transcript = stt.transcribe(&source, self.language.as_deref())?;

        let cmds = transcript
            .captions
            .into_iter()
            .map(|c| {
                let start = Time::from_seconds(c.span.start);
                let duration = Time::from_seconds((c.span.end - c.span.start).max(0.1));
                let layer = Layer::new(
                    "Caption",
                    LayerContent::Text(Box::new(caption_block(&c.text))),
                    duration,
                )
                .at(start);
                Command::AddLayer {
                    parent: None,
                    index: None,
                    layer: Box::new(layer),
                }
            })
            .collect();
        Ok(Command::Batch(cmds))
    }
}

/// "Cut all silences": detect silent spans in the first media layer and replace
/// it with one media layer per *kept* (non-silent) segment — atomically, so the
/// whole operation is a single undo step.
pub struct CutSilences {
    pub threshold_db: f32,
    pub min_silence: f64,
}

impl Default for CutSilences {
    fn default() -> Self {
        Self {
            threshold_db: -35.0,
            min_silence: 0.4,
        }
    }
}

impl Skill for CutSilences {
    fn name(&self) -> &'static str {
        "cut_silences"
    }

    fn plan(&self, ctx: &AgentContext<'_>) -> Result<Command, AgentError> {
        let detector = ctx
            .models
            .silence
            .as_ref()
            .ok_or(MlError::NoProvider("silence_detector"))?;

        let Some(target) = first_media_node(ctx) else {
            return Ok(Command::Batch(Vec::new()));
        };
        let target_id = target.id();
        let source = match &target.content {
            LayerContent::Media { source, .. } => source.clone(),
            _ => return Ok(Command::Batch(Vec::new())),
        };
        let range = target.range;

        let silences = detector.detect(&source, self.threshold_db, self.min_silence)?;
        let kept = complement(
            &silences,
            range.start.as_seconds(),
            range.duration.as_seconds(),
        );

        // Snapshot the clip once; each kept segment is a fresh copy with new ids.
        let base = ctx
            .document
            .scene
            .layer(target_id)
            .ok_or_else(|| AgentError::Skill("target vanished".into()))?;

        let mut cmds = vec![Command::RemoveLayer { id: target_id }];
        for (s, e) in kept {
            let mut piece = base.clone();
            regen_ids(&mut piece);
            piece.start = Time::from_seconds(s);
            piece.duration = Time::from_seconds((e - s).max(0.05));
            // NOTE: a future source-trim command will also shift in/out points;
            // here we only re-place the kept segments on the timeline.
            cmds.push(Command::AddLayer {
                parent: None,
                index: None,
                layer: Box::new(piece),
            });
        }
        Ok(Command::Batch(cmds))
    }
}

// --- helpers ----------------------------------------------------------------

fn first_source(ctx: &AgentContext<'_>) -> Option<String> {
    ctx.document
        .sources
        .first()
        .map(|(id, _)| id.clone())
        .or_else(|| {
            first_media_node(ctx).and_then(|n| match &n.content {
                LayerContent::Media { source, .. } => Some(source.clone()),
                _ => None,
            })
        })
}

fn first_media_node<'a>(ctx: &'a AgentContext<'_>) -> Option<&'a Node> {
    // Search the whole tree (clips usually live under track lanes now), in paint
    // order, so the agent finds media wherever it sits.
    ctx.document
        .scene
        .find_node(|n| matches!(n.content, LayerContent::Media { .. }))
}

/// Give a freshly-cloned subtree brand-new stable ids so copies don't collide.
fn regen_ids(layer: &mut Layer) {
    layer.id = SceneId::new();
    for child in &mut layer.children {
        regen_ids(child);
    }
}

/// Compute the kept (non-silent) spans within `[start, start+duration)`.
fn complement(silences: &[cutlass_ml::Span], start: f64, duration: f64) -> Vec<(f64, f64)> {
    let end = start + duration;
    let mut kept = Vec::new();
    let mut cursor = start;
    for s in silences {
        let s0 = s.start.clamp(start, end);
        let s1 = s.end.clamp(start, end);
        if s0 > cursor {
            kept.push((cursor, s0));
        }
        cursor = cursor.max(s1);
    }
    if cursor < end {
        kept.push((cursor, end));
    }
    kept
}

fn caption_block(text: &str) -> TextBlock {
    TextBlock {
        runs: vec![TextRun {
            text: text.to_string(),
            style: TextStyle {
                font: FontRef {
                    family: "Inter".into(),
                    weight: 600,
                    italic: false,
                },
                align: Align::Center,
                fill: Track::constant(Vec4::ONE),
                size: Track::constant(64.0),
                tracking: Track::constant(0.0),
                line_height: 1.2,
            },
        }],
        width: 1400.0,
        height: None,
    }
}
