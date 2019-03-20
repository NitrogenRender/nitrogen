/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

//! Functionalities for describing and implementing passes.


pub mod command;
pub use self::command::*;

pub mod dispatcher;
pub use self::dispatcher::*;

use crate::graph::{builder, ComputePassAccessor};

use crate::material::MaterialHandle;
use crate::vertex_attrib::VertexAttribHandle;

use crate::util::CowString;

use std::borrow::Cow;
use smallvec::SmallVec;
use std::marker::PhantomData;
use crate::graph::command::ComputeCommandBuffer;

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
#[derive(Clone, Copy, Debug)]
pub enum BlendMode {
    Alpha,
    Add,
    Mul,
}

/// Depth-test mode description.
#[derive(Clone, Copy, Debug)]
pub struct DepthMode {
    /// Function that determines whether the test fails or succeeds.
    pub func: Comparison,
    /// Flag that determines whether depth values are written back or only used for reading/testing.
    pub write: bool,
}

/// Comparison modes used for depth and stencil tests.
#[allow(missing_docs)]
#[derive(Clone, Copy, Debug)]
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

/// Description of a graphics pass pipeline.
#[derive(Default)]
pub struct GraphicsPassInfo {
    /// Vertex-attribute layout used in the pipeline (if any).
    pub vertex_attrib: Option<VertexAttribHandle>,
    /// Depth mode used for a possible depth attachment.
    pub depth_mode: Option<DepthMode>,
    /// Stencil mode used for a possible stencil attachment.
    /// TODO
    pub stencil_mode: Option<()>,
    /// Set of shader programs.
    pub shaders: Shaders,
    /// Primitive mode used for rasterization.
    pub primitive: Primitive,
    /// Blend modes used for the color attachments.
    pub blend_modes: Vec<BlendMode>,
    /// Materials used in the pass with their associated set-bindings.
    pub materials: Vec<(usize, MaterialHandle)>,
    /// Range of push constants used.
    ///
    /// **NOTE**: in 4-bytes, not bytes!!! e.g. 0..4 states bytes 0..16
    pub push_constants: Vec<std::ops::Range<u32>>,
}

/// Description of a compute pass pipeline
#[derive(Default)]
pub struct ComputePassInfo {
    /// Materials used in the pass with their associated set-bindings.
    pub materials: Vec<(usize, MaterialHandle)>,
    /// Description of the compute shader program.
    pub shader: ShaderInfo,
    /// Range of push constants
    pub push_constants: Option<std::ops::Range<u32>>,
}

pub(crate) enum PassInfo {
    Graphics(GraphicsPassInfo),
    Compute(ComputePassInfo),
}

/// Set of shaders used in graphics passes.
///
/// impl-note: TODO add tessellation shaders?
#[derive(Debug, Default)]
pub struct Shaders {
    /// Description of the vertex program. This is mandatory.
    pub vertex: ShaderInfo,
    /// Description of the optional fragment program.
    pub fragment: Option<ShaderInfo>,
    /// Description of the optional geometry program.
    pub geometry: Option<ShaderInfo>,
}

/// Description of a shader program
#[derive(Debug, Clone)]
pub struct ShaderInfo {
    /// SPIR-V binary code of the program.
    pub content: Cow<'static, [u8]>,

    /// The entry point of the program.
    pub entry: CowString,
}

impl Default for ShaderInfo {
    fn default() -> Self {
        ShaderInfo {
            content: Cow::Borrowed(&[]),
            entry: "".into(),
        }
    }
}

/// Trait used to implement graphics pass functionality.
pub trait GraphicsPassImpl {
    /// The `setup` function is called during graph compilation and records all resource
    /// creations and dependencies in the graph-`builder`.
    ///
    /// The `store` can be used to modify the data recorded into `builder`.
    fn setup(&mut self, store: &mut super::Store, builder: &mut builder::GraphBuilder);

    /// The `execute` function is called once for every graph execution.
    ///
    /// Rendering and graphics-queue commands can be recorded into the `cmd`-command buffer.
    ///
    /// Data can be read from the `store` as inputs to the execution.
    unsafe fn execute(&self, store: &super::Store, cmd: &mut command::GraphicsCommandBuffer);
}

#[derive(Debug, Clone, Hash, Eq, PartialEq)]
pub struct Specialization {
    pub id: u32,
    pub value: SmallVec<[u8; 256]>,
}

pub type ShaderHandle = ();

#[derive(Debug, Clone, Hash, Eq, PartialEq)]
pub struct Shader {
    pub handle: ShaderHandle,
    pub specialization: Vec<Specialization>,
}

#[derive(Debug, Clone, Hash, Eq, PartialEq)]
pub struct ComputePipelineInfo {
    pub materials: Vec<(usize, MaterialHandle)>,
    pub push_constant_range: Option<std::ops::Range<u32>>,
    pub shader: Shader,
}

/// Trait used to implement compute pass functionality.
pub trait ComputePass: Sized {

    /// Configuration type of the pass.
    ///
    /// The configuration is used to dispatch work on potentially different pipelines.
    type Config: Sized;

    /// Create a compute-pipeline info
    fn configure(&self, config: Self::Config) -> ComputePipelineInfo;

    /// The `describe` function is called during graph compilation and records all resource
    /// creations and dependencies in the graph-`builder`.
    fn describe(&mut self, res: &mut builder::ResourceDescriptor);

    /// The `execute` function is called once for every graph execution.
    ///
    /// Dispatch commands can be recorded into the `cmd`-command buffer.
    ///
    /// Data can be read from the `store` as inputs to the execution.
    unsafe fn execute(&self, store: &super::Store, cmd: &mut ComputeDispatcher<Self>);
}
