/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

use crate::graph::builder;
use crate::graph::command;

use crate::material::MaterialHandle;
use crate::pipeline::Primitive;
use crate::render_pass::BlendMode;
use crate::vertex_attrib::VertexAttribHandle;

use crate::util::CowString;

use std::borrow::Cow;

#[derive(Copy, Clone, Hash, Eq, PartialEq, Ord, PartialOrd, Debug)]
pub struct PassId(pub(crate) usize);

pub enum PassInfo {
    Graphics {
        vertex_attrib: Option<VertexAttribHandle>,
        shaders: Shaders,
        primitive: Primitive,
        blend_mode: BlendMode,
        materials: Vec<(usize, MaterialHandle)>,
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
