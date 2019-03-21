use crate::graph::execution::PipelineResources;
use crate::graph::pass::command::ComputeCommandBuffer;
use crate::graph::pass::{ComputePass, PassId};
use crate::graph::{ComputePassAccessor, Graph};

pub(crate) struct RawComputeDispatcher<'a> {
    pub(crate) pass_id: PassId,
    pub(crate) graph: &'a mut Graph,
    pub(crate) accessor: &'a ComputePassAccessor,
}

impl<'a> RawComputeDispatcher<'a> {
    pub(crate) fn into_typed_dispatcher<T: ComputePass>(
        self,
        pass_impl: &'a T,
    ) -> ComputeDispatcher<'a, T> {
        ComputeDispatcher {
            pass_id: self.pass_id,
            graph: self.graph,
            accessor: self.accessor,
            pass_impl,
        }
    }
}

pub struct ComputeDispatcher<'a, T: ComputePass> {
    pub(crate) pass_id: PassId,
    pub(crate) graph: &'a mut Graph,
    pub(crate) accessor: &'a ComputePassAccessor,
    pub(crate) pass_impl: &'a T,
}

impl<'a, T: ComputePass> ComputeDispatcher<'a, T> {
    pub unsafe fn with_config<F, R>(&mut self, config: T::Config, f: F) -> R
    where
        F: FnOnce(&mut ComputeCommandBuffer) -> R,
    {
        let desc = self.pass_impl.configure(config);

        // fetch pipeline from cache or create a new one.
        let pipelines = self
            .graph
            .pass_resources
            .compute_pipelines
            .entry(self.pass_id)
            .or_default();

        let pipeline = pipelines
            .entry(desc)
            .or_insert_with(|| unimplemented!("create_pipeline_resources"));

        let mut cmd = unimplemented!("create ComputeCommandBuffer");

        f(&mut cmd)
    }
}
