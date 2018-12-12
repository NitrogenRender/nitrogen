use super::*;
use crate::graph::{GraphResourcesResolved, ResourceName};

use std::collections::HashSet;

pub enum ExecutionGraphError {
    OutputUndefined { name: ResourceName },
}

#[derive(Debug, Clone, Default)]
pub struct ExecutionBatch {
    /// Resources that have to be created from scratch
    pub(crate) resource_create: HashSet<ResourceId>,
    /// Resources that have to be created via copying
    pub(crate) resource_copies: HashSet<ResourceId>,
    /// Passes to execute
    pub(crate) passes: Vec<PassId>,
    /// Resources to destroy
    pub(crate) resource_destroy: HashSet<ResourceId>,
}

#[derive(Debug)]
pub struct ExecutionGraph {
    pub(crate) pass_execution: Vec<ExecutionBatch>,
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
