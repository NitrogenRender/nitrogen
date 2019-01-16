/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

use crate::graph::builder;
use crate::graph::command;

use crate::material::MaterialHandle;
use crate::vertex_attrib::VertexAttribHandle;

use crate::util::CowString;

use std::borrow::Cow;

#[derive(Copy, Clone, Hash, Eq, PartialEq, Ord, PartialOrd, Debug)]
pub struct PassId(pub(crate) usize);

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

#[derive(Clone, Copy, Debug)]
pub enum BlendMode {
    Alpha,
    Add,
    Mul,
}

#[derive(Clone, Copy, Debug)]
pub struct DepthMode {
    pub func: Comparison,
    pub write: bool,
}

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

#[derive(Default)]
pub struct GraphicsPassInfo {
    pub vertex_attrib: Option<VertexAttribHandle>,
    pub depth_mode: Option<DepthMode>,
    // TODO
    pub stencil_mode: Option<()>,
    pub shaders: Shaders,
    pub primitive: Primitive,
    pub blend_modes: Vec<BlendMode>,
    pub materials: Vec<(usize, MaterialHandle)>,
    pub push_constants: Vec<std::ops::Range<u32>>,
}

#[derive(Default)]
pub struct ComputePassInfo {
    pub materials: Vec<(usize, MaterialHandle)>,
    pub shader: ShaderInfo,
    pub push_constants: Vec<std::ops::Range<u32>>,
}

pub(crate) enum PassInfo {
    Graphics(GraphicsPassInfo),
    Compute(ComputePassInfo),
}

#[derive(Debug, Default)]
pub struct Shaders {
    pub vertex: ShaderInfo,
    pub fragment: Option<ShaderInfo>,
    pub geometry: Option<ShaderInfo>,
}

#[derive(Debug, Clone)]
pub struct ShaderInfo {
    pub content: Cow<'static, [u8]>,
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

pub trait GraphicsPassImpl {
    fn setup(&mut self, store: &mut super::Store, builder: &mut builder::GraphBuilder);
    fn execute(&self, store: &super::Store, cmd: &mut command::GraphicsCommandBuffer);
}

pub trait ComputePassImpl {
    fn setup(&mut self, store: &mut super::Store, builder: &mut builder::GraphBuilder);
    fn execute(&self, store: &super::Store, cmd: &mut command::ComputeCommandBuffer);
}
