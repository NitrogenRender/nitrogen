/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

use nitrogen::buffer::BufferUsage;
use nitrogen::submit_group::SubmitGroup;
use nitrogen::*;

fn device_local_buffer_create<T: Sized>(
    ctx: &mut Context,
    submit: &mut SubmitGroup,
    data: &[T],
    usage: buffer::BufferUsage,
) -> Option<buffer::BufferHandle> {
    let usage = usage | BufferUsage::TRANSFER_DST;

    // create buffer
    let buffer = {
        let create_info = buffer::DeviceLocalCreateInfo {
            size: (std::mem::size_of::<T>() * data.len()) as u64,
            usage,
            is_transient: false,
        };

        ctx.buffer_device_local_create(&[create_info])
            .remove(0)
            .ok()?
    };

    // upload
    {
        let upload_info = buffer::BufferUploadInfo { offset: 0, data };

        submit
            .buffer_device_local_upload(ctx, &[(buffer, upload_info)])
            .remove(0)
            .ok()?;
    }

    Some(buffer)
}

pub fn device_local_buffer_vertex<T: Sized>(
    ctx: &mut Context,
    submit: &mut SubmitGroup,
    data: &[T],
) -> Option<buffer::BufferHandle> {
    let usage = BufferUsage::VERTEX;

    device_local_buffer_create(ctx, submit, data, usage)
}

pub fn device_local_buffer_index<T: Sized>(
    ctx: &mut Context,
    submit: &mut SubmitGroup,
    data: &[T],
) -> Option<buffer::BufferHandle> {
    let usage = BufferUsage::INDEX;

    device_local_buffer_create(ctx, submit, data, usage)
}

pub fn device_local_buffer_storage<T: Sized>(
    ctx: &mut Context,
    submit: &mut SubmitGroup,
    data: &[T],
) -> Option<buffer::BufferHandle> {
    let usage = BufferUsage::STORAGE;

    device_local_buffer_create(ctx, submit, data, usage)
}

pub fn device_local_buffer<T: Sized>(
    ctx: &mut Context,
    submit: &mut SubmitGroup,
    data: &[T],
) -> Option<buffer::BufferHandle> {
    let usage = BufferUsage::empty();

    device_local_buffer_create(ctx, submit, data, usage)
}
