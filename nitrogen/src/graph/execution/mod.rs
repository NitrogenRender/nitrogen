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

#[derive(Debug, Default)]
pub(crate) struct GraphBaseResources {
    render_passes: HashMap<PassId, RenderPassHandle>,

    pipelines_graphic: HashMap<PassId, PipelineHandle>,
    pipelines_compute: HashMap<PassId, PipelineHandle>,
    pub(crate) pipelines_mat: HashMap<PassId, crate::material::MaterialHandle>,
}

impl GraphBaseResources {
    pub(crate) fn release(self, res_list: &mut ResourceList, storages: &mut Storages) {
        storages
            .render_pass
            .destroy(res_list, self.render_passes.values());

        storages
            .pipeline
            .destroy(res_list, self.pipelines_graphic.values());

        storages
            .pipeline
            .destroy(res_list, self.pipelines_compute.values());

        for (_, mat) in self.pipelines_mat {
            res_list.queue_material(mat);
        }
    }
}

#[derive(Debug)]
pub(crate) struct GraphResources {
    pub(crate) exec_context: super::ExecutionContext,

    pub(crate) external_resources: HashSet<ResourceId>,
    pub(crate) images: HashMap<ResourceId, ImageHandle>,
    samplers: HashMap<ResourceId, SamplerHandle>,
    pub(crate) buffers: HashMap<ResourceId, BufferHandle>,

    framebuffers: HashMap<PassId, (types::Framebuffer, gfx::image::Extent)>,

    pass_mats: HashMap<PassId, crate::material::MaterialInstanceHandle>,

    pub(crate) outputs: SmallVec<[ResourceId; 16]>,
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

        for (_, (fb, _)) in self.framebuffers {
            res_list.queue_framebuffer(fb);
        }

        for mat_instance in self.pass_mats.values() {
            res_list.queue_material_instance(*mat_instance);
        }
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
