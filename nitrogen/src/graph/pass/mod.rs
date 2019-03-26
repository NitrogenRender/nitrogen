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
use crate::vertex_attrib::VertexAttribHandle;

use crate::resources::shader::{
    ComputeShaderHandle, FragmentShaderHandle, GeometryShaderHandle, VertexShaderHandle,
};
use smallvec::SmallVec;

/// Numerical identifier for a pass.
#[derive(Copy, Clone, Hash, Eq, PartialEq, Ord, PartialOrd, Debug)]
pub struct PassId(pub(crate) usize);

/// Primitive mode used for rasterization.
#[allow(missing_docs)]
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
    pub vertex: Shader<VertexShaderHandle>,
    pub fragment: Option<Shader<FragmentShaderHandle>>,
    pub geometry: Option<Shader<GeometryShaderHandle>>,
}

/// Description of a graphics pass pipeline.
#[derive(Debug, Clone, Hash, Eq, PartialEq)]
pub struct GraphicsPipelineInfo {
    /// Vertex-attribute layout used in the pipeline (if any).
    pub vertex_attrib: Option<VertexAttribHandle>,
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
    ///
    /// **NOTE**: in 4-bytes, not bytes!!! e.g. 0..4 states bytes 0..16
    pub push_constants: Option<std::ops::Range<u32>>,
}

/// Trait used to implement graphics pass functionality.
pub trait GraphicsPass: Sized {
    /// Configuration type of the pass.
    ///
    /// The configuration is used to dispatch work on potentially different pipelines.
    type Config: Sized;

    /// The `prepare` function is called before every execution and can be used to change
    /// pass-internal state.
    fn prepare(&mut self, _store: &mut super::Store) {}

    /// Create a graphics-pipeline info from a given configuration.
    fn configure(&self, config: Self::Config) -> GraphicsPipelineInfo;

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

#[derive(Debug, Clone, Hash, Eq, PartialEq)]
pub struct Specialization {
    pub id: u32,
    pub value: SmallVec<[u8; 256]>,
}

pub fn specialization_constant<T: Copy>(id: u32, data: T) -> Specialization {
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

#[derive(Debug, Clone, Hash, Eq, PartialEq)]
pub struct Shader<HandleType> {
    pub handle: HandleType,
    pub specialization: Vec<Specialization>,
}

#[derive(Debug, Clone, Hash, Eq, PartialEq)]
pub struct ComputePipelineInfo {
    pub materials: Vec<(usize, MaterialHandle)>,
    pub push_constant_range: Option<std::ops::Range<u32>>,
    pub shader: Shader<ComputeShaderHandle>,
}

/// Trait used to implement compute pass functionality.
pub trait ComputePass: Sized {
    /// Configuration type of the pass.
    ///
    /// The configuration is used to dispatch work on potentially different pipelines.
    type Config: Sized;

    /// The `prepare` function is called before every execution and can be used to change
    /// pass-internal state.
    fn prepare(&mut self, _store: &mut super::Store) {}

    /// Create a compute-pipeline info from a given configuration.
    fn configure(&self, config: Self::Config) -> ComputePipelineInfo;

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
