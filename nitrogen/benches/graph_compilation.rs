/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

mod common;

use criterion::{black_box, Criterion};
use criterion::{criterion_group, criterion_main};

#[derive(Copy, Clone)]
enum GraphDependencyType {
    Deep,
    Flat,
}

fn build_graph(num_middle_passes: usize, t: GraphDependencyType) -> nitrogen::graph::GraphBuilder {
    let mut builder = nitrogen::graph::GraphBuilder::new("BenchMark graph");

    {
        struct InitialPass;

        impl nitrogen::graph::ComputePass for InitialPass {
            type Config = ();

            fn configure(&self, config: &Self::Config) -> nitrogen::graph::ComputePipelineInfo {
                unimplemented!()
            }

            fn describe(&mut self, res: &mut nitrogen::graph::ResourceDescriptor) {
                res.virtual_create("Initial");
            }

            unsafe fn execute(
                &self,
                store: &nitrogen::graph::Store,
                dispatcher: &mut nitrogen::graph::ComputeDispatcher<Self>,
            ) -> Result<(), nitrogen::graph::GraphExecError> {
                unimplemented!()
            }
        }

        builder.add_compute_pass("Initial", InitialPass);
    }

    for i in 0..num_middle_passes {
        struct MiddlePass {
            t: GraphDependencyType,
            n: usize,
        }

        impl nitrogen::graph::ComputePass for MiddlePass {
            type Config = ();

            fn configure(&self, config: &Self::Config) -> nitrogen::graph::ComputePipelineInfo {
                unimplemented!()
            }

            fn describe(&mut self, res: &mut nitrogen::graph::ResourceDescriptor) {
                let dependency = match self.t {
                    GraphDependencyType::Deep => {
                        if self.n == 0 {
                            "Initial".to_string()
                        } else {
                            format!("Res{}", self.n - 1)
                        }
                    }
                    GraphDependencyType::Flat => "Initial".to_string(),
                };

                res.virtual_read(dependency);

                res.virtual_create(format!("Res{}", self.n));
            }

            unsafe fn execute(
                &self,
                store: &nitrogen::graph::Store,
                dispatcher: &mut nitrogen::graph::ComputeDispatcher<Self>,
            ) -> Result<(), nitrogen::graph::GraphExecError> {
                unimplemented!()
            }
        }

        builder.add_compute_pass(format!("Middle{}", i), MiddlePass { t, n: i });
    }

    {
        struct LastPass {
            t: GraphDependencyType,
            n: usize,
        }

        impl nitrogen::graph::ComputePass for LastPass {
            type Config = ();

            fn configure(&self, config: &Self::Config) -> nitrogen::graph::ComputePipelineInfo {
                unimplemented!()
            }

            fn describe(&mut self, res: &mut nitrogen::graph::ResourceDescriptor) {
                match self.t {
                    GraphDependencyType::Deep => {
                        // depend on the the very last one
                        res.virtual_read(format!("Res{}", self.n - 1));
                    }
                    GraphDependencyType::Flat => {
                        // depend on all the middle ones
                        for i in 0..self.n {
                            res.virtual_read(format!("Res{}", i));
                        }
                    }
                }

                res.virtual_create("Final");
            }

            unsafe fn execute(
                &self,
                store: &nitrogen::graph::Store,
                dispatcher: &mut nitrogen::graph::ComputeDispatcher<Self>,
            ) -> Result<(), nitrogen::graph::GraphExecError> {
                unimplemented!()
            }
        }

        builder.add_compute_pass(
            "Last",
            LastPass {
                t,
                n: num_middle_passes,
            },
        );
    }

    builder.add_target("Final");

    builder
}

unsafe fn compile_graph(
    ctx: &mut common::BenchContext,
    num_middle_passes: usize,
    t: GraphDependencyType,
) {
    let builder = build_graph(num_middle_passes, t);

    let graph = black_box(ctx.ctx.graph_create(builder).unwrap());

    ctx.group.graph_destroy(&mut ctx.ctx, &[graph]);
}

fn benchmark_graph_compilation(c: &mut Criterion) {
    let ctx = common::BenchContext::new();

    {
        let ctx = ctx.clone();

        c.bench_function_over_inputs(
            "build_graph_deep",
            move |b, i| {
                b.iter(|| unsafe {
                    compile_graph(&mut ctx.borrow_mut(), *i, GraphDependencyType::Deep)
                });
            },
            (0..10).map(|i| 1 << i),
        );
    }

    {
        let ctx = ctx.clone();

        c.bench_function_over_inputs(
            "build_graph_flat",
            move |b, i| {
                b.iter(|| unsafe {
                    compile_graph(&mut ctx.borrow_mut(), *i, GraphDependencyType::Flat)
                });
            },
            (0..10).map(|i| 1 << i),
        );
    }

    common::BenchContext::release(ctx);
}

criterion_group!(bench, benchmark_graph_compilation);
criterion_main!(bench);
