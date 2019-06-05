/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

//! Functionalities for describing and implementing passes.

pub mod command;
pub use self::command::*;

pub mod dispatcher;
pub use self::dispatcher::*;

use crate::graph::{builder, GraphExecError};

use crate::material::MaterialHandle;
use crate::vertex_attrib::VertexAttrib;

use crate::resources::shader::{
    ComputeShaderHandle, FragmentShaderHandle, GeometryShaderHandle, VertexShaderHandle,
};
use smallvec::SmallVec;

use std::hash::Hash;

/// Numerical identifier for a pass.
#[derive(Copy, Clone, Hash, Eq, PartialEq, Ord, PartialOrd, Debug)]
pub struct PassId(pub(crate) usize);

/// Primitive mode used for rasterization.
#[allow(missing_docs)]
#[repr(u8)]
#[derive(Ord, PartialOrd, Eq, PartialEq, Copy, Clone, Hash, Debug)]
pub enum Primitive {
    PointList,
    LineList,
    LineStrip,
    TriangleList,
    TriangleStrip,
}

impl Default for Primitive {
    fn default() -> Self {
        Primitive::TriangleList
    }
}

impl From<Primitive> for gfx::Primitive {
    fn from(p: Primitive) -> Self {
        match p {
            Primitive::PointList => gfx::Primitive::PointList,
            Primitive::LineList => gfx::Primitive::LineList,
            Primitive::LineStrip => gfx::Primitive::LineStrip,
            Primitive::TriangleList => gfx::Primitive::TriangleList,
            Primitive::TriangleStrip => gfx::Primitive::TriangleStrip,
        }
    }
}

// TODO add more modes

/// Blend mode used for color attachments.
#[allow(missing_docs)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BlendMode {
    Alpha,
    Add,
    Mul,
}

/// Depth-test mode description.
#[derive(Debug, Clone, Copy, Hash, Eq, PartialEq)]
pub struct DepthMode {
    /// Function that determines whether the test fails or succeeds.
    pub func: Comparison,
    /// Flag that determines whether depth values are written back or only used for reading/testing.
    pub write: bool,
}

/// Comparison modes used for depth and stencil tests.
#[allow(missing_docs)]
#[derive(Debug, Clone, Copy, Hash, Eq, PartialEq)]
pub enum Comparison {
    Never,
    Less,
    Equal,
    LessEqual,
    Greater,
    NotEqual,
    GreaterEqual,
    Always,
}

impl From<Comparison> for gfx::pso::Comparison {
    fn from(cmp: Comparison) -> Self {
        use self::Comparison as C;
        use gfx::pso::Comparison as GC;
        match cmp {
            C::Never => GC::Never,
            C::Less => GC::Less,
            C::Equal => GC::Equal,
            C::LessEqual => GC::LessEqual,
            C::Greater => GC::Greater,
            C::NotEqual => GC::NotEqual,
            C::GreaterEqual => GC::GreaterEqual,
            C::Always => GC::Always,
        }
    }
}

/// Set of shaders used in graphics passes.
#[derive(Debug, Clone, Hash, Eq, PartialEq)]
pub struct GraphicShaders {
    /// Vertex-shader description.
    pub vertex: Shader<VertexShaderHandle>,
    /// Optional fragment-shader description.
    pub fragment: Option<Shader<FragmentShaderHandle>>,
    /// Optional geometry-shader description.
    pub geometry: Option<Shader<GeometryShaderHandle>>,
}

/// Description of a graphics pass pipeline.
#[derive(Debug, Clone, Hash, Eq, PartialEq)]
pub struct GraphicsPipelineInfo {
    /// Vertex-attribute layout used in the pipeline (if any).
    pub vertex_attrib: Option<VertexAttrib>,
    /// Depth mode used for a possible depth attachment.
    pub depth_mode: Option<DepthMode>,
    /// Stencil mode used for a possible stencil attachment.
    /// TODO
    pub stencil_mode: Option<()>,
    /// Set of shader programs.
    pub shaders: GraphicShaders,
    /// Primitive mode used for rasterization.
    pub primitive: Primitive,
    /// Blend modes used for the color attachments.
    pub blend_modes: Vec<BlendMode>,
    /// Materials used in the pass with their associated set-bindings.
    pub materials: Vec<(usize, MaterialHandle)>,
    /// Range of push constants used.
    pub push_constants: Option<std::ops::Range<u32>>,
}

/// Trait used to implement graphics pass functionality.
pub trait GraphicsPass: Sized {
    /// Configuration type of the pass.
    ///
    /// The configuration is used to dispatch work on potentially different pipelines.
    type Config: Hash + Eq;

    /// The `prepare` function is called before every execution and can be used to change
    /// pass-internal state.
    fn prepare(&mut self, _store: &mut super::Store) {}

    /// Create a graphics-pipeline info from a given configuration.
    fn configure(&self, config: &Self::Config) -> GraphicsPipelineInfo;

    /// The `describe` function is called during graph compilation and records all resource
    /// creations and dependencies in the graph-`builder`.
    fn describe(&mut self, res: &mut builder::ResourceDescriptor);

    /// The `execute` function is called once for every graph execution.
    ///
    /// Dispatch commands can be recorded into the `cmd`-command buffer.
    ///
    /// Data can be read from the `store` as inputs to the execution.
    unsafe fn execute(
        &self,
        store: &super::Store,
        dispatcher: &mut GraphicsDispatcher<Self>,
    ) -> Result<(), GraphExecError>;
}

/// A value description for a specialization constant;
#[derive(Debug, Clone, Hash, Eq, PartialEq)]
pub struct Specialization {
    pub(crate) id: u32,
    pub(crate) value: SmallVec<[u8; 256]>,
}

impl Specialization {
    /// Create a specialization constant for `id` with value `data`.
    pub fn new<T: Copy>(id: u32, data: T) -> Self {
        let data_size = std::mem::size_of::<T>();

        let mut spec_constant = Specialization {
            id,
            value: SmallVec::with_capacity(data_size),
        };

        unsafe {
            spec_constant.value.set_len(data_size);
        }

        let data_ptr = spec_constant.value.as_mut_slice().as_ptr() as *mut T;

        unsafe {
            std::ptr::write_unaligned(data_ptr, data);
        }

        spec_constant
    }
}

/// A description of a shader object used for pipeline-creation.
#[derive(Debug, Clone, Hash, Eq, PartialEq)]
pub struct Shader<HandleType> {
    /// Handle to the shader object.
    pub handle: HandleType,
    /// Specialization constant values to be used in the shader program.
    pub specialization: Vec<Specialization>,
}

/// Description of a compute pass pipeline.
#[derive(Debug, Clone, Hash, Eq, PartialEq)]
pub struct ComputePipelineInfo {
    /// Materials used, described by the descriptor-set binding and a handle to the material.
    pub materials: Vec<(usize, MaterialHandle)>,
    /// Push-constant range (in bytes).
    pub push_constant_range: Option<std::ops::Range<u32>>,
    /// Shader used in this pipeline.
    pub shader: Shader<ComputeShaderHandle>,
}

/// Trait used to implement compute pass functionality.
pub trait ComputePass: Sized {
    /// Configuration type of the pass.
    ///
    /// The configuration is used to dispatch work on potentially different pipelines.
    type Config: Hash + Eq;

    /// The `prepare` function is called before every execution and can be used to change
    /// pass-internal state.
    fn prepare(&mut self, _store: &mut super::Store) {}

    /// Create a compute-pipeline info from a given configuration.
    fn configure(&self, config: &Self::Config) -> ComputePipelineInfo;

    /// The `describe` function is called during graph compilation and records all resource
    /// creations and dependencies in the graph-`builder`.
    fn describe(&mut self, res: &mut builder::ResourceDescriptor);

    /// The `execute` function is called once for every graph execution.
    ///
    /// Dispatch commands can be recorded into the `cmd`-command buffer.
    ///
    /// Data can be read from the `store` as inputs to the execution.
    unsafe fn execute(
        &self,
        store: &super::Store,
        dispatcher: &mut ComputeDispatcher<Self>,
    ) -> Result<(), GraphExecError>;
}
