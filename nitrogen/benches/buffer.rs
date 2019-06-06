/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

mod common;

use common::BenchContext;

use criterion::Criterion;
use criterion::{criterion_group, criterion_main};

unsafe fn create_device_local_buffer(ctx: &mut BenchContext, size: usize) {
    let info = nitrogen::buffer::DeviceLocalCreateInfo {
        size: size as _,
        is_transient: false,
        usage: nitrogen::gfx::buffer::Usage::TRANSFER_SRC
            | nitrogen::gfx::buffer::Usage::TRANSFER_DST,
    };

    let buf = ctx.ctx.buffer_device_local_create(info).unwrap();

    ctx.group.buffer_destroy(&mut ctx.ctx, &[buf]);
    ctx.group.wait(&mut ctx.ctx);
}

unsafe fn create_cpu_visible_buffer(ctx: &mut BenchContext, size: usize) {
    let info = nitrogen::buffer::CpuVisibleCreateInfo {
        size: size as _,
        is_transient: false,
        usage: nitrogen::gfx::buffer::Usage::TRANSFER_SRC
            | nitrogen::gfx::buffer::Usage::TRANSFER_DST,
    };

    let buf = ctx.ctx.buffer_cpu_visible_create(info).unwrap();

    ctx.group.buffer_destroy(&mut ctx.ctx, &[buf]);
    ctx.group.wait(&mut ctx.ctx);
}

fn benchmark_device_local(c: &mut Criterion) {
    let context = BenchContext::new();

    {
        let ctx = context.clone();
        c.bench_function_over_inputs(
            "create device local buffers",
            move |b, i| {
                b.iter(|| unsafe {
                    let mut ctx = ctx.borrow_mut();
                    create_device_local_buffer(&mut ctx, **i);
                })
            },
            &[16, 1024, 1024 * 1024, 1024 * 1024 * 1024],
        );
    }

    BenchContext::release(context);
}

fn benchmark_cpu_visible(c: &mut Criterion) {
    let context = BenchContext::new();

    {
        let ctx = context.clone();
        c.bench_function_over_inputs(
            "create cpu visible buffers",
            move |b, i| {
                b.iter(|| unsafe {
                    let mut ctx = ctx.borrow_mut();
                    create_cpu_visible_buffer(&mut ctx, **i);
                })
            },
            &[16, 1024, 1024 * 1024],
        );
    }

    BenchContext::release(context);
}

criterion_group!(benches, benchmark_device_local, benchmark_cpu_visible);
criterion_main!(benches);
