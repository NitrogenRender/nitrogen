/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

//! Description of vertex buffers and elements used in graphics pipelines.

/// Vertex attribute description.
#[derive(Debug, Clone)]
pub(crate) struct VertexAttribResource {
    /// stride and attributes
    pub(crate) buffers: Vec<VertexBufferDesc>,
    pub(crate) attribs: Vec<gfx::pso::AttributeDesc>,
}

#[derive(Debug, Clone)]
pub(crate) struct VertexBufferDesc {
    pub(crate) stride: usize,
    pub(crate) binding: usize,
}

#[derive(Debug, Clone, Hash, Eq, PartialEq)]
/// Description of vertex information used in a graphics pipeline.
pub struct VertexAttrib {
    /// Description of all buffers used for vertex data.
    pub buffer_infos: Vec<VertexAttribBufferInfo>,
}

#[derive(Debug, Clone, Hash, Eq, PartialEq)]
/// Description of a vertex buffer.
pub struct VertexAttribBufferInfo {
    /// Size in bytes between two vertices.
    pub stride: usize,
    /// Index used when binding a vertex buffer.
    pub index: u32,
    /// Description of the vertex-data.
    pub elements: Vec<VertexAttribBufferElementInfo>,
}

#[derive(Debug, Clone, Hash, Eq, PartialEq)]
/// Description of an element in a vertex buffer.
pub struct VertexAttribBufferElementInfo {
    /// Location used to identify element in shader programs.
    pub location: u32,
    /// Format of the element data.
    pub format: gfx::format::Format,
    /// Offset in bytes inside the buffer.
    pub offset: u32,
}

pub(crate) fn crate_resource(info: &VertexAttrib) -> VertexAttribResource {
    let num_attribs = {
        info.buffer_infos
            .iter()
            .map(|buffer| buffer.elements.len())
            .sum()
    };

    let mut attribs = Vec::with_capacity(num_attribs);
    let mut bufs = Vec::with_capacity(info.buffer_infos.len());

    let attrib_iter = info.buffer_infos.iter().flat_map(|buffer| {
        let index = buffer.index;

        buffer
            .elements
            .iter()
            .map(move |elem| gfx::pso::AttributeDesc {
                location: elem.location,
                binding: index,
                element: gfx::pso::Element {
                    format: elem.format,
                    offset: elem.offset,
                },
            })
    });

    attribs.extend(attrib_iter);

    let bufs_iter = info.buffer_infos.iter().map(|buf_info| VertexBufferDesc {
        stride: buf_info.stride,
        binding: buf_info.index as _,
    });

    bufs.extend(bufs_iter);

    VertexAttribResource {
        buffers: bufs,
        attribs,
    }
}
