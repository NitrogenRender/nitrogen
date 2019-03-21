/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

mod derive;
mod execute;
mod graph;
mod prepare;

pub(crate) use self::derive::*;
pub(crate) use self::execute::*;
pub(crate) use self::graph::*;
pub(crate) use self::prepare::*;

pub use self::prepare::PrepareError;

use super::{PassId, PassName, ResourceId, Storages};
use crate::resources::{
    buffer::BufferHandle, image::ImageHandle, pipeline::PipelineHandle,
    render_pass::RenderPassHandle, sampler::SamplerHandle,
};
use crate::types;

use crate::submit_group::ResourceList;

use std::collections::{HashMap, HashSet};

use smallvec::SmallVec;

use crate::graph::pass::ComputePipelineInfo;
use crate::resources::image::ImageFormat;
use gfx;

/// Errors that can occur when executing a graph.
#[allow(missing_docs)]
#[derive(Clone, Debug, From, Display)]
pub enum GraphExecError {
    #[display(fmt = "Invalid graph handle")]
    InvalidGraph,

    #[display(fmt = "Graph resources could not be created: {}", _0)]
    ResourcePrepareError(PrepareError),
}

impl std::error::Error for GraphExecError {}

#[derive(Debug, Default)]
pub(crate) struct ResourceUsages {
    image: HashMap<ResourceId, (gfx::image::Usage, gfx::format::Format)>,
    buffer: HashMap<ResourceId, gfx::buffer::Usage>,
}

#[derive(Debug)]
pub(crate) struct PipelineResources {
    pub(crate) pipeline_handle: PipelineHandle,
}

#[derive(Debug, Default)]
pub(crate) struct PassResources {
    // Framebuffers have a fixed size. If the size changes then they have to be recreated.
    // Framebuffers do not depend on a pipeline, so they are unaffected by swap of pipeline.
    pub(crate) framebuffers: HashMap<PassId, (types::Framebuffer, gfx::image::Extent)>,
    pub(crate) render_passes: HashMap<PassId, RenderPassHandle>,

    pub(crate) pass_material: HashMap<PassId, crate::material::MaterialInstanceHandle>,
    pub(crate) compute_pipelines: HashMap<PassId, HashMap<ComputePipelineInfo, PipelineResources>>,
}

impl PassResources {
    pub(crate) fn release(self, res_list: &mut ResourceList, storages: &mut Storages) {
        for (_, (fb, _)) in self.framebuffers {
            res_list.queue_framebuffer(fb);
        }

        storages
            .render_pass
            .destroy(res_list, self.render_passes.values());

        for (_, mat_inst) in self.pass_material {
            res_list.queue_material(mat_inst.material);
        }

        for (_, pipes) in self.compute_pipelines {
            let pipes = pipes.values().map(|res| res.pipeline_handle);
            storages.pipeline.destroy(res_list, pipes);
        }
    }
}

#[derive(Debug, Default)]
pub(crate) struct GraphResources {
    pub(crate) exec_context: Option<super::ExecutionContext>,

    pub(crate) external_resources: HashSet<ResourceId>,
    pub(crate) images: HashMap<ResourceId, ImageHandle>,
    samplers: HashMap<ResourceId, SamplerHandle>,
    pub(crate) buffers: HashMap<ResourceId, BufferHandle>,
}

impl GraphResources {
    pub(crate) fn release(self, res_list: &mut ResourceList, storages: &mut Storages) {
        storages.image.destroy(
            res_list,
            self.images.iter().filter_map(|(res, handle)| {
                if self.external_resources.contains(res) {
                    None
                } else {
                    Some(*handle)
                }
            }),
        );

        storages.sampler.destroy(res_list, self.samplers.values());

        storages.buffer.destroy(res_list, self.buffers.values());
    }
}

/// Backbuffers contain resources which can persist graph executions.
#[derive(Debug, Default)]
pub struct Backbuffer {
    pub(crate) usage: BackbufferUsage,

    pub(crate) images: HashMap<super::ResourceName, ImageHandle>,

    // TODO there are no writes to this, only one read. removing??
    pub(crate) samplers: HashMap<super::ResourceName, SamplerHandle>,
}

impl Backbuffer {
    /// Create a new (and empty) backbuffer.
    pub fn new() -> Self {
        Default::default()
    }

    /// Retrieve the handle for an image with the given name from the backbuffer
    pub fn image_get<T: Into<super::ResourceName>>(&self, name: T) -> Option<ImageHandle> {
        self.images.get(&name.into()).cloned()
    }

    /// Insert an image into the Backbuffer with a given name.
    ///
    /// Since an image-handle on its own does not carry format information with it,
    /// the format has to be passed explicitly.
    ///
    /// impl-note: Maybe this should be a method on `Context` instead, so the format can be read
    /// automatically?
    pub fn image_put<T: Into<super::ResourceName>>(
        &mut self,
        name: T,
        image: ImageHandle,
        format: ImageFormat,
    ) {
        let name = name.into();
        self.images.insert(name.clone(), image);
        self.usage.images.insert(name, format.into());
    }
}

#[derive(Debug, Default)]
pub(crate) struct BackbufferUsage {
    pub(crate) images: HashMap<super::ResourceName, gfx::format::Format>,
}
