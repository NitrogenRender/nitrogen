extern crate gfx_backend_vulkan as back;
pub extern crate gfx_hal as gfx;
extern crate gfx_memory as gfxm;

extern crate smallvec;

extern crate failure;
extern crate failure_derive;

extern crate bitflags;

extern crate ash;

extern crate slab;

#[cfg(feature = "winit_support")]
extern crate winit;

use smallvec::SmallVec;

pub mod types;

pub mod display;
use display::Display;

pub mod device;
use device::DeviceContext;

pub mod util;
pub use util::storage;
pub use util::transfer;

pub use util::CowString;

use storage::{Handle, Storage};

pub mod resources;
pub use resources::buffer;
pub use resources::image;
pub use resources::pipeline;
pub use resources::render_pass;
pub use resources::sampler;
pub use resources::vertex_attrib;

pub mod graph;

#[cfg(feature = "x11")]
use ash::vk;

pub type DisplayHandle = Handle<Display>;

// DON'T CHANGE THE ORDER OF THE MEMBERS HERE!!!!
//
// Rust drops structs by dropping the members in declaration order, so things that need to be
// dropped first need to be first in the struct declaration.
//
// BAD THINGS WILL HAPPEN IF YOU CHANGE IT.
// MOUNTAINS OF CRASHES WILL POUR ONTO YOU.
// So please, just don't.
pub struct Context {
    pub graph_storage: graph::GraphStorage,

    pub render_pass_storage: render_pass::RenderPassStorage,
    pub pipeline_storage: pipeline::PipelineStorage,
    pub image_storage: image::ImageStorage,
    pub sampler_storage: sampler::SamplerStorage,
    pub buffer_storage: buffer::BufferStorage,
    pub vertex_attrib_storage: vertex_attrib::VertexAttribStorage,

    pub displays: Storage<Display>,
    pub transfer: transfer::TransferContext,
    pub device_ctx: DeviceContext,
    pub instance: back::Instance,
}

impl Context {
    pub fn new(name: &str, version: u32) -> Self {
        let instance = back::Instance::create(name, version);
        let device_ctx = DeviceContext::new(&instance);

        let transfer = transfer::TransferContext::new(&device_ctx);

        let image_storage = image::ImageStorage::new();
        let sampler_storage = sampler::SamplerStorage::new();
        let buffer_storage = buffer::BufferStorage::new();
        let vertex_attrib_storage = vertex_attrib::VertexAttribStorage::new();
        let pipeline_storage = pipeline::PipelineStorage::new();
        let render_pass_storage = render_pass::RenderPassStorage::new();

        let graph_storage = graph::GraphStorage::new();

        Context {
            instance,
            device_ctx,
            transfer,
            displays: Storage::new(),
            pipeline_storage,
            render_pass_storage,
            image_storage,
            sampler_storage,
            buffer_storage,
            vertex_attrib_storage,
            graph_storage,
        }
    }

    #[cfg(feature = "x11")]
    pub fn add_x11_display(
        &mut self,
        display: *mut vk::Display,
        window: vk::Window,
    ) -> DisplayHandle {
        use gfx::Surface;

        let surface = self.instance.create_surface_from_xlib(display, window);

        let _ = self
            .device_ctx
            .adapter
            .queue_families
            .iter()
            .position(|fam| surface.supports_queue_family(fam))
            .expect("No queue family that supports this surface was found.");

        let display = Display::new(surface, &self.device_ctx);

        self.displays.insert(display).0
    }

    #[cfg(feature = "winit_support")]
    pub fn add_display(&mut self, window: &winit::Window) -> Handle<Display> {
        use gfx::Surface;

        let surface = self.instance.create_surface(window);

        let _ = self
            .device_ctx
            .adapter
            .queue_families
            .iter()
            .position(|fam| surface.supports_queue_family(fam))
            .expect("No queue family that supports this surface was found.");

        let display = Display::new(surface, &self.device_ctx);

        self.displays.insert(display).0
    }

    pub fn remove_display(&mut self, display: DisplayHandle) -> bool {
        match self.displays.remove(display) {
            None => false,
            Some(display) => {
                display.release(&self.device_ctx);
                true
            }
        }
    }

    pub fn release(self) {
        self.buffer_storage.release();
        self.image_storage.release(&self.device_ctx);

        for (_, display) in self.displays {
            display.release(&self.device_ctx);
        }

        self.transfer.release(&self.device_ctx);

        self.device_ctx.release();
    }

    // convenience functions that delegate the work

    // image

    pub fn image_create(
        &mut self,
        create_infos: &[image::ImageCreateInfo],
    ) -> SmallVec<[image::Result<image::ImageHandle>; 16]> {
        self.image_storage.create(&self.device_ctx, create_infos)
    }

    pub fn image_upload_data(
        &mut self,
        images: &[(image::ImageHandle, image::ImageUploadInfo)],
    ) -> SmallVec<[image::Result<()>; 16]> {
        self.image_storage
            .upload_data(&self.device_ctx, &mut self.transfer, images)
    }

    pub fn image_destroy(&mut self, handles: &[image::ImageHandle]) {
        self.image_storage.destroy(&self.device_ctx, handles)
    }

    // sampler

    pub fn sampler_create(
        &mut self,
        create_infos: &[sampler::SamplerCreateInfo],
    ) -> SmallVec<[sampler::SamplerHandle; 16]> {
        self.sampler_storage.create(&self.device_ctx, create_infos)
    }

    pub fn sampler_destroy(&mut self, handles: &[sampler::SamplerHandle]) {
        self.sampler_storage.destroy(&self.device_ctx, handles)
    }

    // vertex attribs

    pub fn vertex_attribs_create(
        &mut self,
        infos: &[vertex_attrib::VertexAttribInfo],
    ) -> SmallVec<[vertex_attrib::VertexAttribHandle; 16]> {
        self.vertex_attrib_storage.create(infos)
    }

    pub fn vertex_attribs_destroy(&mut self, handles: &[vertex_attrib::VertexAttribHandle]) {
        self.vertex_attrib_storage.destroy(handles);
    }

    // old_graph

    pub fn graph_create(&mut self) -> graph::GraphHandle {
        self.graph_storage.create()
    }

    pub fn graph_add_pass<T: Into<graph::PassName>>(
        &mut self,
        graph: graph::GraphHandle,
        name: T,
        info: graph::PassInfo,
        pass_impl: Box<dyn graph::PassImpl>,
    ) {
        self.graph_storage.add_pass(graph, name, info, pass_impl);
    }

    pub fn graph_add_output_image<T: Into<graph::ResourceName>>(&mut self, graph: graph::GraphHandle, image_name: T) {
        self.graph_storage.add_output_image(graph, image_name);
    }

    pub fn graph_add_output_buffer<T: Into<graph::ResourceName>>(&mut self, graph: graph::GraphHandle, buffer_name: T) {
        self.graph_storage.add_output_buffer(graph, buffer_name)
    }

    pub fn graph_destroy(&mut self, graph: graph::GraphHandle) {
        self.graph_storage.destroy(graph);
    }

    pub fn graph_compile(
        &mut self,
        graph: graph::GraphHandle,
    // ) -> Result<(), Vec<graph::constructed::GraphError>> {
    ) -> Result<(), ()> {
        self.graph_storage.compile(graph);
        Ok(())
    }

    pub fn render_graph(
        &mut self,
        graph: graph::GraphHandle,
        exec_context: &graph::ExecutionContext,
    ) {
        /*
        self.graph_storage.execute(
            &self.device_ctx,
            &mut self.render_pass_storage,
            &mut self.pipeline_storage,
            &self.vertex_attrib_storage,
            &mut self.image_storage,
            &mut self.sampler_storage,
            graph,
            exec_context,
        );
        */
    }

    // display

    pub fn display_present(&mut self, display: DisplayHandle, graph_handle: graph::GraphHandle) {
        /*
        if !self.graph_storage.graphs.is_alive(graph_handle) {
            return;
        }

        let graph = &self.graph_storage.graphs[graph_handle];

        if !self
            .graph_storage
            .compiled_graphs
            .contains_key(&graph_handle)
        {
            return;
        }
        let cgraph = &self.graph_storage.compiled_graphs[&graph_handle];

        if !self.graph_storage.resources.contains_key(&graph_handle) {
            return;
        }

        let resources = &self.graph_storage.resources[&graph_handle];

        let (image_handle, sampler_handle) = if let Some(name) = &graph.output_image {
            let id = cgraph.image_name_lookup[name];
            let id = cgraph.resolve_image_id(id).unwrap();
            resources.images[id.0].unwrap()
        } else {
            return;
        };

        self.displays[display].present(
            &self.device_ctx,
            &self.image_storage,
            image_handle,
            &self.sampler_storage,
            sampler_handle,
        );
        */
    }
}
