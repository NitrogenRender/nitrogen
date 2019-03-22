pub mod resource_descriptor;

pub use self::resource_descriptor::*;
use crate::graph::pass::ComputePass;
use crate::graph::{ComputePassAccessor, PassName, ResourceName};
use crate::util::CowString;
use std::cell::RefCell;
use std::rc::Rc;

#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash)]
pub(crate) enum PassType {
    Compute,
    Graphics,
}

pub type GraphName = CowString;

pub struct GraphBuilder {
    pub(crate) name: GraphName,
    pub(crate) compute_passes: Vec<(PassName, ComputePassAccessor)>,
    pub(crate) targets: Vec<ResourceName>,
}

impl GraphBuilder {
    pub fn new(name: impl Into<GraphName>) -> Self {
        GraphBuilder {
            name: name.into(),
            compute_passes: vec![],
            targets: vec![],
        }
    }

    pub fn add_compute_pass(
        &mut self,
        name: impl Into<GraphName>,
        pass: impl ComputePass + 'static,
    ) {
        let accessor = {
            let pass_ref_describe = Rc::new(RefCell::new(pass));
            let pass_ref_execute = pass_ref_describe.clone();

            ComputePassAccessor {
                describe: Box::new(move |res| {
                    pass_ref_describe.borrow_mut().describe(res);
                }),
                execute: Box::new(move |store, dispatcher| {
                    let pass = pass_ref_execute.borrow();
                    {
                        let mut dispatcher =
                            dispatcher.into_typed_dispatcher(pass_ref_execute.clone());

                        unsafe { pass.execute(store, &mut dispatcher) }
                    }
                }),
            }
        };

        self.compute_passes.push((name.into(), accessor));
    }

    pub fn add_target(&mut self, resource_name: impl Into<ResourceName>) {
        self.targets.push(resource_name.into());
    }
}
