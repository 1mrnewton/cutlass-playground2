//! # cutlass-core
//!
//! The single source of truth for a project. The core owns the document,
//! the scene graph, the timeline, undo/redo, and the playback clock; native
//! UIs are thin views that send [`Command`]s and observe state.
//!
//! ## Why "everything is data" matters
//! A **template is just a [`Document`]** with empty media/text slots. Because
//! effects, text, shapes, and animation are all serializable data interpreted
//! by the engine — never platform code — a template authored on a Mac renders
//! identically on Android, iOS, Windows, and Linux. The document format is
//! versioned ([`SCHEMA_VERSION`]) from day one so templates keep opening as the
//! format evolves.

pub mod audio;
pub mod change;
pub mod color;
pub mod command;
pub mod document;
pub mod migrate;
pub mod scene;
pub mod snapshot;
pub mod timewarp;
pub mod track;
pub mod transform;

pub use audio::{db_to_linear, AudioClip, AudioParams, ChannelLayout, FadeCurve};
pub use change::ChangeSet;
pub use color::ColorConfig;
pub use command::{Command, CommandError, Editor};
pub use document::{Document, SCHEMA_VERSION, Template, TemplateSlot};
pub use migrate::DocumentError;
pub use scene::{Layer, LayerContent, Node, NodeId, RemovedLayer, Scene, SceneId};
pub use snapshot::{SceneSnapshot, SharedScene};
pub use timewarp::TimeWarp;
pub use track::{BlendMode, TrackKind, TrackMeta};
pub use transform::Transform2D;

/// Re-export the canonical time types so downstream crates share one `Time`.
pub use cutlass_anim::{Fps, Time, TimeRange};

/// Re-export the canonical color types so downstream crates share one color
/// model (one working space, one set of transfer/gamut definitions).
pub use cutlass_color::{ColorSpace, LinearRgba, Primaries, TransferFunction};
