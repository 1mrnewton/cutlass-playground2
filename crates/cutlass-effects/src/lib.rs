//! # cutlass-effects
//!
//! Effects and filters are **data, not code**: each effect is a node in a DAG
//! of shader passes. The shaders are authored once in WGSL and compiled by
//! `naga` to Metal / Vulkan / D3D12 / GLES, so the *same* shader produces the
//! *same* pixels on every platform. Every parameter is an animatable
//! [`Track`], driven by the shared animation engine.
//!
//! This crate models the graph; the actual GPU execution lives in
//! `cutlass-render`.

use cutlass_anim::Track;
use glam::{Vec2, Vec4};
use serde::{Deserialize, Serialize};

/// A uniform value fed to a shader pass. Each variant is animatable so any
/// effect parameter can be keyframed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Param {
    Float(Track<f32>),
    Vec2(Track<Vec2>),
    Color(Track<Vec4>),
}

/// One shader pass: a WGSL program plus its named parameters and inputs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Pass {
    /// Logical id of the WGSL program in the shader library (`shaders/`).
    pub shader: String,
    pub params: Vec<(String, Param)>,
    /// Indices of upstream nodes in the owning [`EffectGraph`] feeding this
    /// pass (e.g. blur reading the source, then a composite reading both).
    pub inputs: Vec<usize>,
}

/// A DAG of shader passes applied to a layer. Node 0 is conventionally the
/// source; the last node is the graph's output.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct EffectGraph {
    pub nodes: Vec<Pass>,
}

impl EffectGraph {
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// Push a pass and return its node index for wiring downstream inputs.
    pub fn add(&mut self, pass: Pass) -> usize {
        self.nodes.push(pass);
        self.nodes.len() - 1
    }
}
