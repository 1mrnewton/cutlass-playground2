//! # cutlass-agent
//!
//! The AI agent sits **on top of the command bus**. It never mutates the
//! document directly: it inspects the project + media (via [`cutlass_ml`]) and
//! emits a list of [`Command`]s that the engine then executes through the
//! normal [`cutlass_core::Editor`]. That single design choice gives us a lot
//! for free:
//!
//! * **Auditable** — every AI action is an explicit, inspectable command.
//! * **Undoable** — AI edits go through the same undo stack as manual edits.
//! * **Safe** — the agent can only do what the command vocabulary allows.
//! * **Uniform** — the agent is just another command source, like the UI.
//!
//! A user request like *"cut all the silences"* or *"add captions"* becomes a
//! [`Skill`] invocation that returns `Vec<Command>`. An LLM planner (added
//! later) only chooses *which* skills to run and with what arguments — it does
//! not touch engine internals.

mod skills;

pub use skills::{AddCaptions, CutSilences};

use cutlass_core::Command;
use cutlass_ml::{Models, MlError};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AgentError {
    #[error(transparent)]
    Ml(#[from] MlError),
    #[error("skill failed: {0}")]
    Skill(String),
}

/// What a skill can see when planning: the current document plus the model
/// registry. Read-only — skills produce commands, they never mutate.
pub struct AgentContext<'a> {
    pub document: &'a cutlass_core::Document,
    pub models: &'a Models,
}

/// A single capability the agent can invoke. Pure function of context → one
/// atomic [`Command`] (a `Batch` when the edit is several steps), so each skill
/// invocation is exactly one undo step. This is the unit an LLM planner selects
/// and parameterizes.
pub trait Skill {
    /// Stable id used by the planner / tool schema.
    fn name(&self) -> &'static str;
    /// Plan the edit as one atomic engine command.
    fn plan(&self, ctx: &AgentContext<'_>) -> Result<Command, AgentError>;
}

/// The agent: a set of registered skills. `run` returns the commands to apply;
/// the caller (FFI `Session` / `Editor`) executes them so AI edits flow through
/// the exact same path as manual edits.
#[derive(Default)]
pub struct Agent {
    skills: Vec<Box<dyn Skill>>,
}

impl Agent {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, skill: Box<dyn Skill>) -> &mut Self {
        self.skills.push(skill);
        self
    }

    /// Run a named skill and return the atomic command it planned.
    ///
    /// This is the deterministic, placeholder dispatch. The future LLM planner
    /// will map free-form user text to one or more of these calls.
    pub fn run(&self, name: &str, ctx: &AgentContext<'_>) -> Result<Command, AgentError> {
        let skill = self
            .skills
            .iter()
            .find(|s| s.name() == name)
            .ok_or_else(|| AgentError::Skill(format!("unknown skill: {name}")))?;
        skill.plan(ctx)
    }

    /// Names of all registered skills (the agent's "tool list").
    pub fn skill_names(&self) -> Vec<&'static str> {
        self.skills.iter().map(|s| s.name()).collect()
    }
}
