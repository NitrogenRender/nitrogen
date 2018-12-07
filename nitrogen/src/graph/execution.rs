/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

use super::ExecutionContext;
use super::GraphResourcesResolved;
use super::PassId;
use super::PassInfo;
use super::PassName;
use super::ResourceId;
use super::ResourceName;
use super::{ResourceReadType, ResourceWriteType};

use types;
use types::CommandPool;

use std::collections::HashMap;
use std::collections::HashSet;

use gfx;

use gfx::Device;

use smallvec::SmallVec;

use device::DeviceContext;
use resources::{
    buffer::{BufferHandle, BufferStorage},
    image::{ImageHandle, ImageStorage},
    material::MaterialStorage,
    pipeline::{PipelineHandle, PipelineStorage},
    render_pass::{RenderPassHandle, RenderPassStorage},
    sampler::{SamplerHandle, SamplerStorage},
    semaphore_pool::{SemaphoreList, SemaphorePool},
    vertex_attrib::VertexAttribStorage,
};
use submit_group::ResourceList;

#[derive(Debug, Clone)]
pub struct ExecutionBatch {
    /// Resources that have to be created from scratch
    resource_create: HashSet<ResourceId>,
    /// Resources that have to be created via copying
    resource_copies: HashSet<ResourceId>,
    /// Passes to execute
    passes: Vec<PassId>,
    /// Resources to destroy
    resource_destroy: HashSet<ResourceId>,
}

#[derive(Debug)]
pub struct ExecutionGraph {
    pass_execution: Vec<ExecutionBatch>,
}

pub enum ExecutionGraphError {
    OutputUndefined { name: ResourceName },
}

impl ExecutionGraph {
    pub(crate) fn new(resolved: &GraphResourcesResolved, outputs: &[ResourceName]) -> Self {
        let mut pass_execs: Vec<Vec<PassId>> = vec![];

        let mut needed_resources = HashSet::with_capacity(outputs.len());

        let mut errors = vec![];

        let outputs = outputs
            .iter()
            .filter_map(|res_name| match resolved.name_lookup.get(res_name) {
                None => {
                    errors.push(ExecutionGraphError::OutputUndefined {
                        name: res_name.clone(),
                    });
                    None
                }
                Some(id) => Some(*id),
            })
            .collect::<HashSet<_>>();

        // We keep a list of things we should **not** destroy.
        // At the time of this writing, the only special case is the original
        // resources of outputs.
        //
        // (This is because the "origins" of moved resources must not be destroyed
        //  when they are in an output position. Generally moved resources are not destroyed,
        //  only the "origins")
        //
        // I hope that anybody who touches this code will update this comment
        // in case new options are added.
        let mut keep_list = HashSet::new();
        {
            keep_list.extend(outputs.iter().cloned());

            for output in &outputs {
                let mut prev_id = *output;
                while let Some(id) = resolved.moves_from.get(&prev_id) {
                    keep_list.insert(*id);
                    prev_id = *id;
                }
            }
        }

        // Insert initial resources that we want.
        for output in &outputs {
            needed_resources.insert(*output);
        }

        let mut next_passes = HashSet::new();

        while !needed_resources.is_empty() {
            // find passes that create the resource
            for res in &needed_resources {
                next_passes.insert(resolved.defines[res]);
            }

            // Emit passes
            pass_execs.push(next_passes.iter().cloned().collect());

            // We know the passes, which means we don't care about the individual resources anymore
            needed_resources.clear();

            // Find resources that are needed in order for the passes to execute
            for pass in &next_passes {
                for res in &resolved.pass_ext_depends[pass] {
                    needed_resources.insert(*res);
                }
            }

            // Now we know the resources, so we no longer care about the past-passes
            next_passes.clear();
        }

        // When walking the graph, we went from the output up all the dependencies,
        // which means that the list we have is actually backwards!
        // We would like to know which passes to execute first.
        pass_execs.reverse();

        // We need no futher resources \o/
        // That means the list is done, but the list might contain duplicated passes.
        //
        // The list could look like this:
        // [[0, 1], [2, 0], [3]]
        //   => "3 depends on 0 and 2, but 2 depends on 1 and 0"
        //
        // So in this example you can see that the 0 in the middle doesn't need to be there.
        // In fact, every node that was enountered once does not need to be in the list at a
        // later point.
        //
        // Here we use a HashSet to keep track of all previously encountered nodes and then
        // remove all duplicates.
        let pass_execs = {
            let mut known_nodes = HashSet::new();

            pass_execs
                .into_iter()
                .map(|batch| {
                    let deduped = batch
                        .into_iter()
                        .filter(|pass| !known_nodes.contains(pass))
                        .collect::<Vec<_>>();

                    for pass in &deduped {
                        known_nodes.insert(*pass);
                    }

                    deduped
                })
                .collect::<Vec<_>>()
        };

        // We have a list of passes to execute, but those passes also create resources.
        // We can determine at which point the resources have to be created and are free to be
        // destroyed.
        let exec_list = {
            use std::collections::HashMap;
            let mut last_use = HashMap::new();

            for batch in &pass_execs {
                for pass in batch {
                    for res in &resolved.pass_creates[pass] {
                        last_use.insert(*res, *pass);
                    }

                    for dep in &resolved.pass_ext_depends[pass] {
                        last_use.insert(*dep, *pass);
                    }
                }
            }

            let mut pass_destroys = HashMap::new();

            for (res, pass) in last_use {
                pass_destroys
                    .entry(pass)
                    .or_insert(HashSet::new())
                    .insert(res);
            }

            pass_execs
                .into_iter()
                .map(|batch| {
                    let (creates, copies, deletes) = {
                        let all_creates = batch
                            .iter()
                            .filter_map(|pass| resolved.pass_creates.get(pass))
                            .flatten();

                        let creates = all_creates
                            .clone()
                            // We really only care about *new* things that are created.
                            // (no copies or moves)
                            .filter(|res| resolved.infos.contains_key(res))
                            .cloned()
                            .collect();

                        let copies = all_creates
                            // Here we are only interested in the things we need to copy
                            .filter(|res| resolved.copies_from.contains_key(res))
                            .cloned()
                            .collect();

                        let deletes = batch
                            .iter()
                            .filter_map(|pass| pass_destroys.get(pass))
                            .flatten()
                            // If a resource was created by moving the original
                            .filter_map(|res| resolved.moved_from(*res).or(Some(*res)))
                            .filter(|res| !keep_list.contains(res))
                            // Also don't destroy output resources. Ever.
                            .collect();

                        (creates, copies, deletes)
                    };

                    ExecutionBatch {
                        resource_create: creates,
                        resource_copies: copies,
                        resource_destroy: deletes,
                        passes: batch,
                    }
                })
                .collect()
        };

        ExecutionGraph {
            pass_execution: exec_list,
        }
    }
}

/// Resources for a single variant of an execution graph
pub(crate) struct ExecutionGraphResources {
    pub render_passes_graphic: HashMap<PassId, RenderPassHandle>,

    pub pipelines_graphic: HashMap<PassId, PipelineHandle>,
    pub pipelines_graphic_desc_sets:
        HashMap<PassId, (types::DescriptorSetLayout, types::DescriptorSet)>,

    pub usages: Option<ResourceUsages>,
}

impl ExecutionGraphResources {
    pub(crate) fn release(self, device: &DeviceContext, storages: &mut ExecutionStorages) {
        let passes = self
            .render_passes_graphic
            .into_iter()
            .map(|(_, render_pass)| render_pass)
            .collect::<SmallVec<[_; 16]>>();

        storages.render_pass.destroy(device, passes.as_slice());
    }
}

pub struct ExecutionResources {
    pub(crate) outputs: Vec<ResourceId>,

    pub images: HashMap<ResourceId, ImageHandle>,
    pub samplers: HashMap<ResourceId, SamplerHandle>,
    pub buffers: HashMap<ResourceId, BufferHandle>,
    framebuffers: HashMap<PassId, types::Framebuffer>,
}

impl ExecutionResources {
    pub fn release(
        self,
        res_list: &mut ResourceList,
        image: &mut ImageStorage,
        sampler: &mut SamplerStorage,
        buffer: &mut BufferStorage,
    ) {
        use smallvec::SmallVec;

        {
            let images = self.images.values().cloned().collect::<SmallVec<[_; 16]>>();
            image.destroy(res_list, images.as_slice());
        }

        {
            let samplers = self
                .samplers
                .values()
                .cloned()
                .collect::<SmallVec<[_; 16]>>();
            sampler.destroy(res_list, samplers.as_slice());
        }

        {
            let buffers = self
                .buffers
                .values()
                .cloned()
                .collect::<SmallVec<[_; 16]>>();
            buffer.destroy(res_list, buffers.as_slice());
        }
    }
}

pub(crate) struct ExecutionStorages<'a> {
    pub render_pass: &'a mut RenderPassStorage,
    pub pipeline: &'a mut PipelineStorage,
    pub image: &'a mut ImageStorage,
    pub buffer: &'a mut BufferStorage,
    pub vertex_attrib: &'a VertexAttribStorage,
    pub sampler: &'a mut SamplerStorage,
    pub material: &'a MaterialStorage,
}

#[derive(Debug)]
pub(crate) struct ResourceUsages {
    image: HashMap<ResourceId, (gfx::image::Usage, gfx::format::Format)>,
    buffer: HashMap<ResourceId, (gfx::buffer::Usage, gfx::memory::Properties)>,
}

pub(crate) fn derive_resource_usage(
    exec_graph: &ExecutionGraph,
    resolved: &GraphResourcesResolved,
) -> ResourceUsages {
    let mut usages = ResourceUsages {
        image: HashMap::new(),
        buffer: HashMap::new(),
    };

    use gfx::buffer::Usage as BUsage;
    use gfx::image::Usage as IUsage;
    use gfx::memory::Properties;

    use super::ResourceCreateInfo;

    use super::BufferStorageType;

    for batch in &exec_graph.pass_execution {
        for create in &batch.resource_create {
            let info = &resolved.infos[create];

            match info {
                ResourceCreateInfo::Buffer(buf) => {
                    let usage = BUsage::empty();
                    let properties = match buf.storage {
                        BufferStorageType::HostVisible => Properties::CPU_VISIBLE,
                        BufferStorageType::DeviceLocal => Properties::DEVICE_LOCAL,
                    };

                    usages.buffer.insert(*create, (usage, properties));
                }
                ResourceCreateInfo::Image(img) => {
                    let format = img.format.into();
                    let usage = IUsage::empty();

                    usages.image.insert(*create, (usage, format));
                }
            }
        }

        for copy in &batch.resource_copies {
            let orig = &resolved.copies_from[copy];

            if let Some((mut usage, format)) = usages.image.get(orig).map(|x| x.clone()) {
                let mut orig_usage = usage;

                orig_usage |= IUsage::TRANSFER_SRC;
                usages
                    .image
                    .get_mut(orig)
                    .map(move |entry| entry.0 = orig_usage);

                // Once we copy we get rid of all previous flags, since they no longer apply
                usage = IUsage::TRANSFER_DST;
                usages.image.insert(*copy, (usage, format));
            }

            if let Some((mut usage, prop)) = usages.buffer.get(orig).map(|x| x.clone()) {
                let mut orig_usage = usage;

                orig_usage |= BUsage::TRANSFER_SRC;
                usages
                    .buffer
                    .get_mut(orig)
                    .map(move |entry| entry.0 = orig_usage);

                // Once we copy we get rid of all previous flags, since they no longer apply
                usage = BUsage::TRANSFER_DST;
                usages.buffer.insert(*copy, (usage, prop));
            }
        }

        for pass in &batch.passes {
            for (res, read_ty, _, _) in &resolved.pass_reads[pass] {
                let origin = resolved.moved_from(*res).unwrap();

                use super::BufferReadType;
                use super::ImageReadType;

                match read_ty {
                    ResourceReadType::Buffer(buf) => {
                        let (mut usage, prop) = usages.buffer[&origin].clone();

                        match buf {
                            BufferReadType::Storage => {
                                usage |= BUsage::STORAGE;
                            }
                            BufferReadType::StorageTexel => {
                                usage |= BUsage::STORAGE_TEXEL;
                            }
                            BufferReadType::Uniform => {
                                usage |= BUsage::UNIFORM;
                            }
                            BufferReadType::UniformTexel => {
                                usage |= BUsage::UNIFORM_TEXEL;
                            }
                        }

                        usages.buffer.insert(origin, (usage, prop));
                    }
                    ResourceReadType::Image(img) => {
                        let (mut usage, format) = usages.image[&origin].clone();

                        match img {
                            ImageReadType::Color => {
                                usage |= IUsage::SAMPLED;
                            }
                            ImageReadType::Storage => {
                                usage |= IUsage::STORAGE;
                            }
                        }

                        usages.image.insert(origin, (usage, format));
                    }
                }
            }

            for (res, write_ty, _) in &resolved.pass_writes[pass] {
                let origin = resolved.moved_from(*res).unwrap();

                use super::BufferWriteType;
                use super::ImageWriteType;

                match write_ty {
                    ResourceWriteType::Buffer(buf) => {
                        let (mut usage, prop) = usages.buffer[&origin].clone();

                        match buf {
                            BufferWriteType::Storage => {
                                usage |= BUsage::STORAGE;
                            }
                            BufferWriteType::StorageTexel => {
                                usage |= BUsage::STORAGE_TEXEL;
                            }
                        }

                        usages.buffer.insert(origin, (usage, prop));
                    }
                    ResourceWriteType::Image(img) => {
                        let (mut usage, format) = usages.image[&origin].clone();

                        match img {
                            ImageWriteType::Color => {
                                usage |= IUsage::COLOR_ATTACHMENT;
                            }
                            ImageWriteType::DepthStencil => {
                                usage |= IUsage::DEPTH_STENCIL_ATTACHMENT;
                            }
                            ImageWriteType::Storage => {
                                usage |= IUsage::STORAGE;
                            }
                        }

                        usages.image.insert(origin, (usage, format));
                    }
                }
            }
        }
    }

    usages
}

pub(crate) fn prepare(
    device: &DeviceContext,
    storages: &mut ExecutionStorages,
    exec_graph: &ExecutionGraph,
    resolved_graph: &GraphResourcesResolved,
    passes: &[(PassName, PassInfo)],
    outputs: &[ResourceId],
    _context: &ExecutionContext,
) -> ExecutionGraphResources {
    let mut res = ExecutionGraphResources {
        render_passes_graphic: HashMap::new(),
        pipelines_graphic: HashMap::new(),

        pipelines_graphic_desc_sets: HashMap::new(),

        usages: None,
    };

    {
        let usages = res
            .usages
            .get_or_insert_with(|| derive_resource_usage(exec_graph, resolved_graph));

        // output resources must be readable (either via sampling or being a transfer src)
        {
            // TODO handle this properly. What if it's a storage image??
            /*
            usages.image.iter_mut()
                .filter(|(res, _)| outputs.contains(*res))
                .filter_map(|(res, format)| resolved_graph.moved_from(*res))
                .for_each(|(res, (usage, _))| {
                    *usage |= gfx::image::Usage::SAMPLED;
                    *usage |= gfx::image::Usage::TRANSFER_SRC;
                });
                */
            outputs
                .iter()
                .filter_map(|res| resolved_graph.moved_from(*res))
                .for_each(|res| {
                    usages.image.get_mut(&res).map(|(usage, _)| {
                        *usage |= gfx::image::Usage::SAMPLED;
                        *usage |= gfx::image::Usage::TRANSFER_SRC;
                    });
                });

            // TODO do buffers
        }

        for batch in &exec_graph.pass_execution {
            for pass in &batch.passes {
                let info = &passes[pass.0].1;

                // render pass
                let render_pass =
                    create_render_pass_graphics(device, storages, resolved_graph, *pass, info);
                if let Some(handle) = &render_pass {
                    res.render_passes_graphic.insert(*pass, *handle);
                }

                // pipeline
                if let Some(handle) = &render_pass {
                    let graphics_pipeline = create_pipeline_graphics(
                        device,
                        storages,
                        resolved_graph,
                        *handle,
                        *pass,
                        info,
                    );
                    if let Some((handle, layout, set)) = graphics_pipeline {
                        res.pipelines_graphic.insert(*pass, handle);
                        res.pipelines_graphic_desc_sets.insert(*pass, (layout, set));
                    }
                }
            }
        }
    }

    res
}

pub(crate) fn execute(
    device: &DeviceContext,
    sem_pool: &mut SemaphorePool,
    sem_list: &mut SemaphoreList,
    cmd_pool: &mut CommandPool<gfx::Graphics>,
    res_list: &mut ResourceList,
    storages: &mut ExecutionStorages,
    exec_graph: &ExecutionGraph,
    resolved_graph: &GraphResourcesResolved,
    graph: &super::Graph,
    resources: &ExecutionGraphResources,
    context: &ExecutionContext,
) -> ExecutionResources {
    let outputs = graph
        .output_resources
        .iter()
        .filter_map(|name| resolved_graph.name_lookup.get(name))
        .cloned()
        .collect();

    let mut res = ExecutionResources {
        images: HashMap::new(),
        buffers: HashMap::new(),
        samplers: HashMap::new(),
        framebuffers: HashMap::new(),
        outputs,
    };

    for batch in &exec_graph.pass_execution {
        // create new resources
        {
            for create in &batch.resource_create {
                // TODO do something special if this is in the backbuffer

                use super::ResourceCreateInfo;
                let info = &resolved_graph.infos[create];

                use image;
                use sampler;

                match info {
                    ResourceCreateInfo::Image(img) => {
                        let dimension = match img.size_mode {
                            image::ImageSizeMode::Absolute { width, height } => {
                                image::ImageDimension::D2 {
                                    x: width,
                                    y: height,
                                }
                            }
                            image::ImageSizeMode::ContextRelative { width, height } => {
                                image::ImageDimension::D2 {
                                    x: (width as f64 * context.reference_size.0 as f64) as u32,
                                    y: (height as f64 * context.reference_size.1 as f64) as u32,
                                }
                            }
                        };

                        let kind = image::ViewKind::D2;

                        let format = img.format;

                        let usages = &resources.usages.as_ref().unwrap().image[create];

                        let image_create_info = image::ImageCreateInfo {
                            dimension,
                            num_layers: 1,
                            num_samples: 1,
                            num_mipmaps: 1,
                            format,
                            kind,
                            usage: usages.0.clone(),
                            is_transient: true,
                        };

                        let img_handle = storages
                            .image
                            .create(device, &[image_create_info])
                            .remove(0)
                            .unwrap();

                        res.images.insert(*create, img_handle);

                        if usages.0.contains(gfx::image::Usage::SAMPLED) {
                            let sampler = storages
                                .sampler
                                .create(
                                    device,
                                    &[sampler::SamplerCreateInfo {
                                        min_filter: sampler::Filter::Linear,
                                        mip_filter: sampler::Filter::Linear,
                                        mag_filter: sampler::Filter::Linear,
                                        wrap_mode: (
                                            sampler::WrapMode::Clamp,
                                            sampler::WrapMode::Clamp,
                                            sampler::WrapMode::Clamp,
                                        ),
                                    }],
                                )
                                .remove(0);

                            res.samplers.insert(*create, sampler);
                        }
                    }
                    ResourceCreateInfo::Buffer(_buf) => unimplemented!(),
                }
            }
        }

        // create framebuffers
        {
            for pass in &batch.passes {
                let render_pass = &resources.render_passes_graphic[pass];
                let render_pass = storages.render_pass.raw(*render_pass).unwrap();

                let (views, dims): (SmallVec<[_; 16]>, SmallVec<[_; 16]>) = {
                    // Do attachments have to be sorted? I assume so but I should really check the
                    // vulkan spec since gfx doesn't say much about it... TODO

                    let mut sorted_attachment = resolved_graph.pass_writes[pass]
                        .iter()
                        .collect::<SmallVec<[_; 16]>>();

                    sorted_attachment
                        .as_mut_slice()
                        .sort_by_key(|(_, _, binding)| binding);

                    sorted_attachment
                        .into_iter()
                        .map(|(res_id, _ty, _binding)| {
                            let res_id = resolved_graph.moved_from(*res_id).unwrap();

                            // TODO use a

                            let handle = res.images[&res_id];

                            let image = storages.image.raw(handle).unwrap();

                            (&image.view, &image.dimension)
                        })
                        .unzip()
                };

                let extent = {
                    use image;
                    dims.as_slice()
                        .iter()
                        .map(|img_dim| match img_dim {
                            image::ImageDimension::D1 { x } => (*x, 1, 1),
                            image::ImageDimension::D2 { x, y } => (*x, *y, 1),
                            image::ImageDimension::D3 { x, y, z } => (*x, *y, *z),
                        })
                        .map(|(x, y, z)| gfx::image::Extent {
                            width: x,
                            height: y,
                            depth: z,
                        })
                        .next()
                        .unwrap()
                };

                let framebuffer = device
                    .device
                    .create_framebuffer(render_pass, views, extent)
                    .unwrap();

                res.framebuffers.insert(*pass, framebuffer);
            }
        }

        // perform copies
        {}

        // execute passes
        {
            let read_storages = super::command::ReadStorages {
                buffer: storages.buffer,
                material: storages.material,
            };

            for _ in 0..batch.passes.len() {
                let sem = sem_pool.alloc();
                sem_list.add_next_semaphore(sem);
            }

            // TODO FEARLESS CONCURRENCY!!!
            for pass in &batch.passes {
                let pipeline = {
                    let handle = resources.pipelines_graphic[pass];
                    storages.pipeline.raw_graphics(handle).unwrap()
                };

                let render_pass = {
                    let handle = resources.render_passes_graphic[pass];
                    storages.render_pass.raw(handle).unwrap()
                };

                let (_set_layout, set) = &resources.pipelines_graphic_desc_sets[pass];

                // TODO transition resource layouts

                // fill descriptor set
                {
                    let writes = resolved_graph.pass_reads[pass]
                        .iter()
                        .map(|(rid, ty, binding, samp)| {
                            use super::ImageReadType;

                            match ty {
                                ResourceReadType::Image(img) => {
                                    let img_handle = &res.images[rid];
                                    let image = storages.image.raw(*img_handle).unwrap();

                                    match img {
                                        ImageReadType::Color => {
                                            let samp_handle = &res.samplers[rid];
                                            let sampler =
                                                storages.sampler.raw(*samp_handle).unwrap();

                                            let img_desc = gfx::pso::DescriptorSetWrite {
                                                set,
                                                binding: (*binding) as u32,
                                                array_offset: 0,
                                                descriptors: std::iter::once(
                                                    gfx::pso::Descriptor::Image(
                                                        &image.view,
                                                        gfx::image::Layout::General,
                                                    ),
                                                ),
                                            };

                                            let sampler_desc = gfx::pso::DescriptorSetWrite {
                                                set,
                                                binding: samp.clone().unwrap() as u32,
                                                array_offset: 0,
                                                descriptors: std::iter::once(
                                                    gfx::pso::Descriptor::Sampler(sampler),
                                                ),
                                            };

                                            let mut vec: SmallVec<[_; 2]> = SmallVec::new();
                                            vec.push(img_desc);
                                            vec.push(sampler_desc);

                                            vec
                                        }
                                        ImageReadType::Storage => {
                                            let desc = gfx::pso::DescriptorSetWrite {
                                                set,
                                                binding: (*binding) as u32,
                                                array_offset: 0,
                                                descriptors: std::iter::once(
                                                    gfx::pso::Descriptor::Image(
                                                        &image.view,
                                                        gfx::image::Layout::General,
                                                    ),
                                                ),
                                            };

                                            let mut res: SmallVec<[_; 2]> = SmallVec::new();
                                            res.push(desc);

                                            res
                                        }
                                    }
                                }
                                ResourceReadType::Buffer(_buf) => unimplemented!(),
                            }
                        })
                        .flatten();

                    device.device.write_descriptor_sets(writes);
                }

                let framebuffer = &res.framebuffers[pass];

                let framebuffer_extent = context.reference_size; // TODO get actual framebuffer size

                let viewport = gfx::pso::Viewport {
                    depth: 0.0..1.0,
                    rect: gfx::pso::Rect {
                        x: 0,
                        y: 0,
                        w: framebuffer_extent.0 as i16,
                        h: framebuffer_extent.1 as i16,
                    },
                };

                let clear_values = &[gfx::command::ClearValue::Color(
                    gfx::command::ClearColor::Float([0.0, 0.0, 0.0, 0.0]),
                )];

                let submit = {
                    let mut raw_cmd = cmd_pool.acquire_command_buffer(false);
                    raw_cmd.bind_graphics_pipeline(&pipeline.pipeline);

                    raw_cmd.set_viewports(0, &[viewport.clone()]);
                    raw_cmd.set_scissors(0, &[viewport.rect]);

                    raw_cmd.bind_graphics_descriptor_sets(&pipeline.layout, 0, Some(set), &[]);

                    let pass_impl = &graph.passes_impl[pass.0];

                    {
                        let encoder = raw_cmd.begin_render_pass_inline(
                            render_pass,
                            framebuffer,
                            viewport.rect,
                            clear_values,
                        );
                        let mut command = super::command::CommandBuffer {
                            encoder,
                            storages: &read_storages,
                            pipeline_layout: &pipeline.layout,
                        };

                        pass_impl.execute(&mut command);
                    }

                    raw_cmd.finish()
                };

                {
                    let submission = gfx::Submission::new()
                        .wait_on(
                            sem_pool
                                .list_prev_sems(sem_list)
                                .map(|sem| (sem, gfx::pso::PipelineStage::BOTTOM_OF_PIPE)),
                        )
                        .signal(sem_pool.list_next_sems(sem_list))
                        .submit(Some(submit));
                    device.graphics_queue().submit(submission, None);
                }

                sem_list.advance();
            }
        }

        // destroy resources
        {
            // destroy framebuffers first
            for pass in &batch.passes {
                let framebuffer = res.framebuffers.remove(pass).unwrap();

                res_list.queue_framebuffer(framebuffer);
            }

            // destroy images and samplers
            for res_id in &batch.resource_destroy {
                if let Some(img) = res.images.remove(res_id) {
                    let sampler = res.samplers.remove(res_id);
                    if let Some(sampler) = sampler {
                        storages.sampler.destroy(res_list, &[sampler]);
                    }

                    storages.image.destroy(res_list, &[img]);
                }
            }
        }
    }

    res
}

fn create_render_pass_graphics(
    device: &DeviceContext,
    storages: &mut ExecutionStorages,
    resolved_graph: &GraphResourcesResolved,
    pass: PassId,
    _info: &PassInfo,
) -> Option<RenderPassHandle> {
    let attachments = {
        resolved_graph.pass_writes[&pass]
            .iter()
            // we are only interested in images that are written to as color or depth
            .filter(|(_, ty, _)| {
                use super::ImageWriteType;

                match ty {
                    ResourceWriteType::Image(ImageWriteType::Color) => true,
                    ResourceWriteType::Image(ImageWriteType::DepthStencil) => true,
                    _ => false,
                }
            })
            .map(|(res, _ty, _binding)| {
                let (origin, info) = resolved_graph.create_info(*res).unwrap();

                use super::ResourceCreateInfo;

                let info = match info {
                    ResourceCreateInfo::Image(img) => img,
                    _ => unreachable!(),
                };

                let clear = origin == *res;

                let load_op = if clear {
                    gfx::pass::AttachmentLoadOp::Clear
                } else {
                    gfx::pass::AttachmentLoadOp::Load
                };

                let initial_layout = if clear {
                    gfx::image::Layout::Undefined
                } else {
                    gfx::image::Layout::Preinitialized
                };

                gfx::pass::Attachment {
                    format: Some(info.format.into()),
                    samples: 0,
                    ops: gfx::pass::AttachmentOps {
                        load: load_op,
                        store: gfx::pass::AttachmentStoreOp::Store,
                    },
                    // TODO stencil and depth
                    stencil_ops: gfx::pass::AttachmentOps::DONT_CARE,
                    // TODO depth/stencil
                    // TODO Better layout transitions
                    layouts: initial_layout..gfx::image::Layout::General,
                }
            })
            .collect::<SmallVec<[_; 16]>>()
    };

    let color_attachments = attachments
        .as_slice()
        .iter()
        .enumerate()
        .map(|(i, _)| (i, gfx::image::Layout::ColorAttachmentOptimal))
        .collect::<SmallVec<[_; 16]>>();

    let subpass = gfx::pass::SubpassDesc {
        colors: color_attachments.as_slice(),
        // TODO OOOOOOOOOO
        depth_stencil: None,
        inputs: &[],
        resolves: &[],
        preserves: &[],
    };

    let dependencies = gfx::pass::SubpassDependency {
        passes: gfx::pass::SubpassRef::External..gfx::pass::SubpassRef::Pass(0),
        stages: gfx::pso::PipelineStage::COLOR_ATTACHMENT_OUTPUT
            ..gfx::pso::PipelineStage::COLOR_ATTACHMENT_OUTPUT,
        accesses: gfx::image::Access::empty()
            ..(gfx::image::Access::COLOR_ATTACHMENT_READ
                | gfx::image::Access::COLOR_ATTACHMENT_WRITE),
    };

    use render_pass::RenderPassCreateInfo;

    let create_info = RenderPassCreateInfo {
        attachments: attachments.as_slice(),
        subpasses: &[subpass],
        dependencies: &[dependencies],
    };

    storages
        .render_pass
        .create(device, &[create_info])
        .remove(0)
        .ok()
}

fn create_pipeline_graphics(
    device: &DeviceContext,
    storages: &mut ExecutionStorages,
    resolved_graph: &GraphResourcesResolved,
    render_pass: RenderPassHandle,
    pass: PassId,
    info: &PassInfo,
) -> Option<(
    PipelineHandle,
    types::DescriptorSetLayout,
    types::DescriptorSet,
)> {
    use std::collections::BTreeMap;

    let (primitive, shaders, vertex_attribs, materials) = match info {
        PassInfo::Graphics {
            primitive,
            shaders,
            vertex_attrib,
            materials,
            ..
        } => (*primitive, shaders, vertex_attrib, materials),
        _ => unreachable!(),
    };

    use pipeline;

    let (layouts, pass_stuff) = {
        use super::{BufferReadType, ImageReadType, ResourceReadType};

        let mut sets = BTreeMap::new();

        let (core_desc, core_range) = {
            let reads = resolved_graph.pass_reads[&pass].iter();

            let samplers = reads.clone().filter(|(_, _, _, sampler)| sampler.is_some());

            let sampler_descriptors =
                samplers
                    .clone()
                    .map(|(_, _, _, binding)| gfx::pso::DescriptorSetLayoutBinding {
                        binding: binding.unwrap() as u32,
                        ty: gfx::pso::DescriptorType::Sampler,
                        count: 1,
                        stage_flags: gfx::pso::ShaderStageFlags::ALL,
                        immutable_samplers: false,
                    });

            let descriptors = reads
                .clone()
                .map(|(_res, ty, binding, _)| {
                    gfx::pso::DescriptorSetLayoutBinding {
                        binding: (*binding as u32),
                        ty: match ty {
                            ResourceReadType::Image(img) => match img {
                                ImageReadType::Color => gfx::pso::DescriptorType::SampledImage,
                                ImageReadType::Storage => gfx::pso::DescriptorType::StorageImage,
                            },
                            ResourceReadType::Buffer(buf) => {
                                match buf {
                                    BufferReadType::Uniform => {
                                        gfx::pso::DescriptorType::UniformBuffer
                                    }
                                    BufferReadType::UniformTexel => {
                                        // TODO test this
                                        // does this need samplers? I think so. Let's find out!
                                        gfx::pso::DescriptorType::UniformTexelBuffer
                                    }
                                    BufferReadType::Storage => {
                                        gfx::pso::DescriptorType::StorageBuffer
                                    }
                                    BufferReadType::StorageTexel => {
                                        //  TODO test this
                                        // does this need samplers? I think so. Let's find out!
                                        gfx::pso::DescriptorType::StorageTexelBuffer
                                    }
                                }
                            }
                        },
                        count: 1,
                        stage_flags: gfx::pso::ShaderStageFlags::ALL,
                        immutable_samplers: false,
                    }
                })
                .chain(sampler_descriptors);

            let range = reads
                .map(|(_, ty, _, _)| {
                    gfx::pso::DescriptorRangeDesc {
                        ty: match ty {
                            ResourceReadType::Image(img) => match img {
                                ImageReadType::Color => gfx::pso::DescriptorType::SampledImage,
                                ImageReadType::Storage => gfx::pso::DescriptorType::StorageImage,
                            },
                            ResourceReadType::Buffer(buf) => {
                                match buf {
                                    BufferReadType::Uniform => {
                                        gfx::pso::DescriptorType::UniformBuffer
                                    }
                                    BufferReadType::UniformTexel => {
                                        // TODO test this
                                        // does this need samplers? I think so. Let's find out!
                                        gfx::pso::DescriptorType::UniformTexelBuffer
                                    }
                                    BufferReadType::Storage => {
                                        gfx::pso::DescriptorType::StorageBuffer
                                    }
                                    BufferReadType::StorageTexel => {
                                        //  TODO test this
                                        // does this need samplers? I think so. Let's find out!
                                        gfx::pso::DescriptorType::StorageTexelBuffer
                                    }
                                }
                            }
                        },
                        count: 1,
                    }
                })
                .chain(samplers.map(|_| gfx::pso::DescriptorRangeDesc {
                    ty: gfx::pso::DescriptorType::Sampler,
                    count: 1,
                }));

            (descriptors, range)
        };

        // material sets
        {
            for (set, material) in materials {
                let mat = match storages.material.raw(*material) {
                    Some(mat) => mat,
                    None => continue,
                };

                let layout = &mat.desc_set_layout;

                sets.insert(*set, layout);
            }
        }

        use gfx::DescriptorPool;
        use gfx::Device;

        let pass_set_layout = device
            .device
            .create_descriptor_set_layout(core_desc, &[])
            .ok()?;

        let mut pass_set_pool = device.device.create_descriptor_pool(1, core_range).ok()?;

        let pass_set = pass_set_pool.allocate_set(&pass_set_layout).ok()?;

        (sets, (pass_set_layout, pass_set_pool, pass_set))
    };

    let pipeline_handle = {
        let mut layouts = layouts;

        // insert the pass set layout
        layouts.insert(0, &pass_stuff.0);

        let layouts = layouts
            .into_iter()
            .map(|(_, data)| {
                // TODO warning, this is super duper unsafe
                // Currently gfx doesn't let us clone descriptor set layouts.
                // This is the only way around it right now. Argh.
                // use std::mem;

                data
                // unsafe {
                //     mem::transmute_copy(data)
                // }
            })
            .collect::<Vec<_>>();

        let create_info = pipeline::GraphicsPipelineCreateInfo {
            vertex_attribs: vertex_attribs.clone(),
            primitive,
            shader_vertex: pipeline::ShaderInfo {
                content: &shaders.vertex.content,
                entry: &shaders.vertex.entry,
            },
            shader_fragment: if shaders.fragment.is_some() {
                Some(pipeline::ShaderInfo {
                    content: &shaders.fragment.as_ref().unwrap().content,
                    entry: &shaders.fragment.as_ref().unwrap().entry,
                })
            } else {
                None
            },
            // TODO add support for geometry shaders
            shader_geometry: None,
            descriptor_set_layout: &layouts[..],
        };

        storages
            .pipeline
            .create_graphics_pipelines(
                device,
                storages.render_pass,
                storages.vertex_attrib,
                render_pass,
                &[create_info],
            )
            .remove(0)
            .ok()
    };

    pipeline_handle.map(move |handle| (handle, pass_stuff.0, pass_stuff.2))
}
