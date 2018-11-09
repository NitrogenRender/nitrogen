use graph::builder;
use graph::command;

use pipeline::Primitive;
use render_pass::BlendMode;
use vertex_attrib::VertexAttribHandle;

use util::CowString;

use std::borrow::Cow;

#[derive(Copy, Clone, Hash, Eq, PartialEq, Ord, PartialOrd, Debug)]
pub struct PassId(pub(crate) usize);

pub enum PassInfo {
    Graphics {
        vertex_attrib: Option<VertexAttribHandle>,
        shaders: Shaders,
        primitive: Primitive,
        blend_mode: BlendMode,
    },
    Compute {},
}

pub struct Shaders {
    pub vertex: ShaderInfo,
    pub fragment: Option<ShaderInfo>,
    pub geometry: Option<ShaderInfo>,
}

pub struct ShaderInfo {
    pub content: Cow<'static, [u8]>,
    pub entry: CowString,
}

pub trait PassImpl {
    fn setup(&mut self, builder: &mut builder::GraphBuilder);
    fn execute(&self, command_buffer: &mut command::CommandBuffer);
}
