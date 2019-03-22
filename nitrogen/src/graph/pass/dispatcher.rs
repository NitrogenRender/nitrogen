use crate::device::DeviceContext;
use crate::graph::execution::{create_pipeline_compute, PassResources, PipelineResources};
use crate::graph::pass::command::{ComputeCommandBuffer, ReadStorages};
use crate::graph::pass::{ComputePass, PassId};
use crate::graph::{ComputePassAccessor, Graph, PrepareError, Storages};
use crate::resources::pipeline::PipelineError;
use std::cell::RefCell;
use std::rc::Rc;

pub(crate) struct RawComputeDispatcher<'a> {
    pub(crate) cmd: &'a mut crate::resources::command_pool::CmdBufType<gfx::Compute>,
    pub(crate) device: &'a DeviceContext,
    pub(crate) storages: &'a Storages<'a>,
    pub(crate) pass_id: PassId,
    pub(crate) res: &'a mut PassResources,
}

impl<'a> RawComputeDispatcher<'a> {
    pub(crate) fn into_typed_dispatcher<T: ComputePass>(
        self,
        pass_impl: Rc<RefCell<T>>,
    ) -> ComputeDispatcher<'a, T> {
        ComputeDispatcher {
            cmd: self.cmd,
            device: self.device,
            storages: self.storages,
            pass_id: self.pass_id,
            res: self.res,
            pass_impl,
        }
    }
}

pub struct ComputeDispatcher<'a, T: ComputePass> {
    pub(crate) cmd: &'a mut crate::resources::command_pool::CmdBufType<gfx::Compute>,
    pub(crate) device: &'a DeviceContext,
    pub(crate) storages: &'a Storages<'a>,
    pub(crate) pass_id: PassId,
    pub(crate) res: &'a mut PassResources,
    pub(crate) pass_impl: Rc<RefCell<T>>,
}

impl<'a, T: ComputePass> ComputeDispatcher<'a, T> {
    pub unsafe fn with_config<F, R>(&mut self, config: T::Config, f: F) -> Result<R, PrepareError>
    where
        F: FnOnce(&mut ComputeCommandBuffer) -> R,
    {
        let desc = self.pass_impl.borrow().configure(config);

        // fetch pipeline from cache or create a new one.
        let compute_pipelines = &mut self.res.compute_pipelines;
        let pass_materials = &mut self.res.pass_material;

        let pipelines = compute_pipelines.entry(self.pass_id).or_default();

        if !pipelines.contains_key(&desc) {
            // create new pipeline!
            let pass_mat = pass_materials.get(&self.pass_id).map(|mat| mat.material());

            let pipe =
                create_pipeline_compute(self.device, self.storages, self.pass_id, pass_mat, &desc)?;

            pipelines.insert(
                desc.clone(),
                PipelineResources {
                    pipeline_handle: pipe,
                },
            );
        }

        let pipeline_storage = self.storages.pipeline.borrow();

        let read_storages = ReadStorages {
            buffer: self.storages.buffer.borrow(),
            material: self.storages.material.borrow(),
            image: self.storages.image.borrow(),
        };

        let pipe = pipelines.get(&desc).unwrap();

        let pipe_raw = { pipeline_storage.raw_compute(pipe.pipeline_handle).unwrap() };

        self.cmd.bind_compute_pipeline(&pipe_raw.pipeline);

        let mut cmd = {
            ComputeCommandBuffer {
                buf: self.cmd,
                storages: &read_storages,
                pipeline_layout: &pipe_raw.layout,
            }
        };

        let res = f(&mut cmd);

        Ok(res)
    }
}
