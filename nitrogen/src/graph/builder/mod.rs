/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

//! Builder object for creating and describing graphs.

pub mod resource_descriptor;

pub use self::resource_descriptor::*;
use crate::graph::pass::{ComputePass, GraphicsPass};
use crate::graph::{ComputePassAccessor, GraphicPassAccessor, PassName, ResourceName};
use crate::util::CowString;
use std::cell::RefCell;
use std::rc::Rc;

#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash)]
pub(crate) enum PassType {
    Compute,
    Graphics,
}

/// Name of a graph.
pub type GraphName = CowString;

/// Object used to create graphs.
pub struct GraphBuilder {
    pub(crate) name: GraphName,
    pub(crate) compute_passes: Vec<(PassName, ComputePassAccessor)>,
    pub(crate) graphic_passes: Vec<(PassName, GraphicPassAccessor)>,
    pub(crate) targets: Vec<ResourceName>,
}

impl GraphBuilder {
    /// Create a new `GraphBuilder` for a graph named `name`.
    pub fn new(name: impl Into<GraphName>) -> Self {
        GraphBuilder {
            name: name.into(),
            compute_passes: vec![],
            graphic_passes: vec![],
            targets: vec![],
        }
    }

    /// Add a compute pass to the graph with a given name.
    pub fn add_compute_pass(
        &mut self,
        name: impl Into<GraphName>,
        pass: impl ComputePass + 'static,
    ) {
        // Because the ComputePass trait has an associated type, it can not be used like
        // `dyn ComputePass`.
        // To get around this, a list of "accessor closures" are provided which hide the concrete
        // type of the pass.
        let accessor = {
            let pass_ref_prepare = Rc::new(RefCell::new(pass));
            let pass_ref_describe = pass_ref_prepare.clone();
            let pass_ref_execute = pass_ref_prepare.clone();

            ComputePassAccessor {
                prepare: Box::new(move |store| {
                    pass_ref_prepare.borrow_mut().prepare(store);
                }),
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

    /// Add a graphics pass to the graph with a given name.
    pub fn add_graphics_pass(
        &mut self,
        name: impl Into<GraphName>,
        pass: impl GraphicsPass + 'static,
    ) {
        // Because the GraphicsPass trait has an associated type, it can not be used like
        // `dyn ComputePass`.
        // To get around this, a list of "accessor closures" are provided which hide the concrete
        // type of the pass.
        let accessor = {
            let pass_ref_prepare = Rc::new(RefCell::new(pass));
            let pass_ref_describe = pass_ref_prepare.clone();
            let pass_ref_execute = pass_ref_prepare.clone();

            GraphicPassAccessor {
                prepare: Box::new(move |store| {
                    pass_ref_prepare.borrow_mut().prepare(store);
                }),
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

        self.graphic_passes.push((name.into(), accessor));
    }

    /// Add a "target resource" to the graph.
    ///
    /// A target resource can be used as an "output" of the graph and is used to determine the
    /// final graph execution order.
    pub fn add_target(&mut self, resource_name: impl Into<ResourceName>) {
        self.targets.push(resource_name.into());
    }
}
