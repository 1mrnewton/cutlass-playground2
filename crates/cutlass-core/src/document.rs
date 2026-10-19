//! The project/template document: the portable, versioned unit that the whole
//! template ecosystem is built on.

use crate::color::ColorConfig;
use crate::migrate::{self, DocumentError};
use crate::scene::{Scene, SceneId};
use serde::{Deserialize, Serialize};

/// Document schema version. Bump on any breaking change and append the matching
/// migration in [`crate::migrate`] so old templates keep opening. Versioning
/// from commit #1 is non-negotiable for a template ecosystem.
pub const SCHEMA_VERSION: u32 = 1;

/// A complete project: metadata, the scene, and the list of bundled fonts /
/// media sources it depends on (so a template carries everything it needs).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Document {
    pub schema_version: u32,
    pub title: String,
    pub scene: Scene,
    /// The project's color pipeline (working + output spaces). Defaulted so
    /// pre-color documents keep loading as SDR sRGB.
    #[serde(default)]
    pub color: ColorConfig,
    /// Logical font family -> bundled font file (relative to the project pack).
    #[serde(default)]
    pub fonts: Vec<(String, String)>,
    /// Logical source id -> media file (relative to the project pack).
    #[serde(default)]
    pub sources: Vec<(String, String)>,
    /// Present when this document is a template.
    #[serde(default)]
    pub template: Option<Template>,
}

impl Document {
    pub fn new(title: impl Into<String>, scene: Scene) -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            title: title.into(),
            scene,
            color: ColorConfig::default(),
            fonts: Vec::new(),
            sources: Vec::new(),
            template: None,
        }
    }

    /// Serialize to JSON at the current [`SCHEMA_VERSION`] (the "save" path).
    pub fn to_json(&self) -> Result<String, DocumentError> {
        Ok(serde_json::to_string_pretty(self)?)
    }

    /// Parse JSON written by *any* supported schema version (the "load" path):
    /// the raw value is migrated forward to [`SCHEMA_VERSION`] before it is
    /// deserialized into the current runtime types. Documents from a newer build
    /// are rejected with [`DocumentError::FromFuture`] rather than silently
    /// mis-parsed.
    pub fn from_json(s: &str) -> Result<Self, DocumentError> {
        let value = migrate::load_value(s)?;
        Ok(serde_json::from_value(value)?)
    }
}

/// Template metadata layered on top of a [`Document`]. A template is an
/// ordinary document plus declared *slots* the user fills (their clips, their
/// text). Filling slots is what "applying a template" means.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Template {
    pub author: String,
    pub category: String,
    pub slots: Vec<TemplateSlot>,
}

/// A user-fillable hole in a template, bound to a specific scene node.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TemplateSlot {
    pub label: String,
    pub target: SceneId,
    pub kind: SlotKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SlotKind {
    Media,
    Text,
    Color,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::{Layer, LayerContent, Scene};
    use cutlass_anim::{Fps, Time};

    #[test]
    fn json_round_trip_through_arena_dto() {
        let mut scene = Scene::new(1920, 1080, Fps::from_int(30));
        scene.duration = Time::from_seconds(5.0);
        let parent = Layer::new("Group", LayerContent::Group, Time::from_seconds(5.0));
        let pid = parent.id;
        scene.insert_root(parent);
        scene.insert(
            Some(pid),
            None,
            Layer::new("Child", LayerContent::Group, Time::from_seconds(2.0)),
        )
        .unwrap();

        let doc = Document::new("RoundTrip", scene);
        let json = doc.to_json().unwrap();
        let back = Document::from_json(&json).unwrap();

        // Semantic equality (arena keys may differ; logical tree must not).
        assert_eq!(doc, back);
        assert_eq!(back.scene.len(), 2);
        assert_eq!(back.scene.roots().len(), 1);
    }

    fn small_doc() -> Document {
        Document::new("Probe", Scene::new(64, 64, Fps::from_int(24)))
    }

    /// A document missing `schema_version` (predates the field) loads as v1 and
    /// is stamped with the current version.
    #[test]
    fn loads_legacy_document_without_schema_version() {
        let mut v: serde_json::Value = serde_json::from_str(&small_doc().to_json().unwrap()).unwrap();
        v.as_object_mut().unwrap().remove("schema_version");
        let back = Document::from_json(&serde_json::to_string(&v).unwrap()).unwrap();
        assert_eq!(back.schema_version, SCHEMA_VERSION);
        assert_eq!(back.title, "Probe");
    }

    /// A document from a future schema is rejected, not mis-parsed.
    #[test]
    fn rejects_document_from_the_future() {
        let mut v: serde_json::Value = serde_json::from_str(&small_doc().to_json().unwrap()).unwrap();
        v["schema_version"] = serde_json::json!(999);
        let err = Document::from_json(&serde_json::to_string(&v).unwrap()).unwrap_err();
        assert!(matches!(err, crate::DocumentError::FromFuture { found: 999, .. }));
    }

    /// Forward-compat within a version: an unknown extra field is ignored, not an
    /// error (we don't `deny_unknown_fields`).
    #[test]
    fn unknown_fields_are_ignored_within_a_version() {
        let mut v: serde_json::Value = serde_json::from_str(&small_doc().to_json().unwrap()).unwrap();
        v["a_field_from_the_future"] = serde_json::json!({ "x": 1 });
        let back = Document::from_json(&serde_json::to_string(&v).unwrap()).unwrap();
        assert_eq!(back.title, "Probe");
    }
}
