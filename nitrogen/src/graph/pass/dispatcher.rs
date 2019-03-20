use crate::graph::ComputePassAccessor;
use crate::graph::pass::ComputePass;
use std::marker::PhantomData;
use crate::graph::pass::command::ComputeCommandBuffer;

pub(crate) struct RawComputeDispatcher<'a> {
    pub(crate) accessor: &'a ComputePassAccessor,
}

impl<'a> RawComputeDispatcher<'a> {
    pub(crate) fn into_typed_dispatcher<T: ComputePass>(self, pass_impl: &'a T) -> ComputeDispatcher<'a, T> {
        ComputeDispatcher {
            accessor: self.accessor,
            pass_impl,
            _marker: PhantomData,
        }
    }
}


pub struct ComputeDispatcher<'a, T: ComputePass> {

    pub(crate) accessor: &'a ComputePassAccessor,
    pub(crate) pass_impl: &'a T,

    _marker: PhantomData<&'a T>,
}

impl<'a, T: ComputePass> ComputeDispatcher<'a, T> {

    pub fn with_config<F, R>(&mut self, config: T::Config, f: F) -> R
        where F: FnOnce(&mut ComputeCommandBuffer) -> R,
    {
        let desc = self.pass_impl.configure(config);

        // fetch pipeline from cache or create a new one.

        let mut cmd = unimplemented!();

        f(&mut cmd)
    }

}

