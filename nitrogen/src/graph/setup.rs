use std::collections::HashMap;

use super::PassId;
use super::ResourceId;
use super::Graph;

pub(crate) struct SetUpGraph {

}

impl SetUpGraph {
    pub fn new(graph: &mut Graph) -> Self {

        // let mut created_resources = HashMap::new();

        for (i, pass_impl) in graph.passes_impl.iter_mut().enumerate() {
            let pass_id = PassId(i);

            let mut builder = super::GraphBuilder::new();
            pass_impl.setup(&mut builder);

            if builder.enabled {
                // TODO
            }
        }

        SetUpGraph {}
    }
}