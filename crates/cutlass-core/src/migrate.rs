//! Versioned document loading with **forward migration**.
//!
//! The on-disk format and the in-memory structs are deliberately *not* welded
//! together. Loading is a pipeline:
//!
//! ```text
//! raw JSON ─parse─▶ serde_json::Value ─migrate v(n)→v(n+1)*─▶ deserialize ─▶ Document
//! ```
//!
//! Saving always writes the current [`SCHEMA_VERSION`]. Every breaking format
//! change appends exactly one [`Step`] to [`MIGRATIONS`] and bumps
//! `SCHEMA_VERSION`, so a project authored on any past version keeps opening —
//! the whole point of versioning a template ecosystem from commit #1.
//!
//! ## Forward-compatibility policy
//! *Within* a schema version the format is **additive**: we do **not**
//! `deny_unknown_fields`, and new fields carry `#[serde(default)]`. So a document
//! written by a slightly newer build (same `schema_version`, one extra optional
//! field) still loads on an older build, and a *breaking* change is the only
//! thing that bumps the version + adds a migration.

use crate::document::SCHEMA_VERSION;
use serde_json::Value;
use thiserror::Error;

/// Everything that can go wrong loading or saving a [`Document`](crate::Document).
#[derive(Debug, Error)]
pub enum DocumentError {
    #[error("document json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("document schema v{found} is newer than this build supports (v{supported}); please update the app")]
    FromFuture { found: u32, supported: u32 },
    #[error("no migration registered from schema v{from} (cannot reach v{target})")]
    NoMigration { from: u32, target: u32 },
    #[error("migrating from schema v{from}: {detail}")]
    Migration { from: u32, detail: String },
}

/// One forward migration: rewrite a document `Value` from version `from` to
/// `from + 1`, in place.
struct Step {
    from: u32,
    apply: fn(&mut Value) -> Result<(), DocumentError>,
}

/// The ordered production migration chain. **Empty at v1.** Each future breaking
/// change appends one `Step { from: N, .. }` and bumps `SCHEMA_VERSION` to `N+1`.
/// The engine ([`run`]) is generic over the step set so it is fully tested even
/// while this is empty.
const MIGRATIONS: &[Step] = &[];

/// Read the embedded `schema_version`, defaulting to 1 for documents written
/// before the field existed.
fn version_of(value: &Value) -> u32 {
    value
        .get("schema_version")
        .and_then(Value::as_u64)
        .map(|n| n as u32)
        .unwrap_or(1)
}

/// Bring a parsed document `Value` up to `target` by applying `steps` in order.
fn run(value: &mut Value, steps: &[Step], target: u32) -> Result<(), DocumentError> {
    let mut v = version_of(value);
    if v > target {
        return Err(DocumentError::FromFuture {
            found: v,
            supported: target,
        });
    }
    while v < target {
        let step = steps
            .iter()
            .find(|s| s.from == v)
            .ok_or(DocumentError::NoMigration { from: v, target })?;
        (step.apply)(value)?;
        v += 1;
    }
    // Guarantee the embedded version matches what we will deserialize as (also
    // stamps documents that predate the field).
    if let Some(obj) = value.as_object_mut() {
        obj.insert("schema_version".into(), Value::from(target));
    }
    Ok(())
}

/// Parse + migrate a document JSON string into a current-version `Value`, ready
/// to deserialize into [`Document`](crate::Document).
pub(crate) fn load_value(s: &str) -> Result<Value, DocumentError> {
    let mut value: Value = serde_json::from_str(s)?;
    run(&mut value, MIGRATIONS, SCHEMA_VERSION)?;
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // Synthetic steps exercising the engine without inventing a fake *production*
    // migration (production stays empty until a real breaking change lands).
    fn rename_title_to_name(v: &mut Value) -> Result<(), DocumentError> {
        let obj = v.as_object_mut().ok_or(DocumentError::Migration {
            from: 1,
            detail: "not an object".into(),
        })?;
        if let Some(t) = obj.remove("title") {
            obj.insert("name".into(), t);
        }
        Ok(())
    }
    fn add_flag(v: &mut Value) -> Result<(), DocumentError> {
        if let Some(obj) = v.as_object_mut() {
            obj.insert("flag".into(), Value::Bool(true));
        }
        Ok(())
    }

    #[test]
    fn chains_steps_in_order_and_stamps_target_version() {
        let steps = [
            Step {
                from: 1,
                apply: rename_title_to_name,
            },
            Step {
                from: 2,
                apply: add_flag,
            },
        ];
        let mut v = json!({ "schema_version": 1, "title": "Hi" });
        run(&mut v, &steps, 3).unwrap();
        assert_eq!(v["schema_version"], json!(3));
        assert_eq!(v["name"], json!("Hi"), "1->2 renamed the field");
        assert!(v.get("title").is_none());
        assert_eq!(v["flag"], json!(true), "2->3 added the field");
    }

    #[test]
    fn missing_version_is_treated_as_v1() {
        let steps = [Step {
            from: 1,
            apply: add_flag,
        }];
        let mut v = json!({ "title": "x" }); // no schema_version
        run(&mut v, &steps, 2).unwrap();
        assert_eq!(v["schema_version"], json!(2));
        assert_eq!(v["flag"], json!(true));
    }

    #[test]
    fn future_version_is_rejected() {
        let mut v = json!({ "schema_version": 9 });
        let err = run(&mut v, &[], 1).unwrap_err();
        assert!(matches!(
            err,
            DocumentError::FromFuture {
                found: 9,
                supported: 1
            }
        ));
    }

    #[test]
    fn a_gap_in_the_chain_is_an_error() {
        let steps = [Step {
            from: 2,
            apply: add_flag,
        }]; // 1->2 missing
        let mut v = json!({ "schema_version": 1 });
        let err = run(&mut v, &steps, 3).unwrap_err();
        assert!(matches!(
            err,
            DocumentError::NoMigration { from: 1, target: 3 }
        ));
    }

    #[test]
    fn current_version_is_a_noop() {
        let mut v = json!({ "schema_version": 1, "title": "x" });
        run(&mut v, &[], 1).unwrap();
        assert_eq!(v, json!({ "schema_version": 1, "title": "x" }));
    }
}
