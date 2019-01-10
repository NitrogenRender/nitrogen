/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

use nitrogen::buffer::BufferUsage;
use nitrogen::submit_group::SubmitGroup;
use nitrogen::*;

pub unsafe fn buffer_device_local_create<T: Sized>(
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

pub unsafe fn buffer_device_local_vertex<T: Sized>(
    ctx: &mut Context,
    submit: &mut SubmitGroup,
    data: &[T],
) -> Option<buffer::BufferHandle> {
    let usage = BufferUsage::VERTEX;

    buffer_device_local_create(ctx, submit, data, usage)
}

pub unsafe fn buffer_device_local_index<T: Sized>(
    ctx: &mut Context,
    submit: &mut SubmitGroup,
    data: &[T],
) -> Option<buffer::BufferHandle> {
    let usage = BufferUsage::INDEX;

    buffer_device_local_create(ctx, submit, data, usage)
}

pub unsafe fn buffer_device_local_storage<T: Sized>(
    ctx: &mut Context,
    submit: &mut SubmitGroup,
    data: &[T],
) -> Option<buffer::BufferHandle> {
    let usage = BufferUsage::STORAGE;

    buffer_device_local_create(ctx, submit, data, usage)
}

pub unsafe fn image_create(
    ctx: &mut Context,
    dimensions: (u32, u32),
    format: image::ImageFormat,
    usage: gfx::image::Usage,
) -> Option<image::ImageHandle> {
    let dimension = image::ImageDimension::D2 {
        x: dimensions.0,
        y: dimensions.1,
    };

    let create_info = image::ImageCreateInfo {
        dimension,
        num_layers: 1,
        num_samples: 1,
        num_mipmaps: 1,

        format,
        kind: image::ViewKind::D2,
        is_transient: false,

        usage,
    };

    ctx.image_create(&[create_info]).remove(0).ok()
}

pub unsafe fn image_create_with_content(
    ctx: &mut Context,
    submit: &mut SubmitGroup,
    content: &[u8],
    dimensions: (u32, u32),
    format: image::ImageFormat,
    usage: gfx::image::Usage,
) -> Option<(image::ImageHandle, sampler::SamplerHandle)> {
    let dimension = image::ImageDimension::D2 {
        x: dimensions.0,
        y: dimensions.1,
    };

    let img = image_create(ctx, dimensions, format, usage)?;

    let upload = image::ImageUploadInfo {
        data: content,
        format,
        dimension,
        target_offset: (0, 0, 0),
    };

    submit
        .image_upload_data(ctx, &[(img, upload)])
        .remove(0)
        .ok()?;

    let sampler_create = sampler::SamplerCreateInfo {
        min_filter: sampler::Filter::Linear,
        mag_filter: sampler::Filter::Linear,
        mip_filter: sampler::Filter::Linear,
        wrap_mode: (
            sampler::WrapMode::Clamp,
            sampler::WrapMode::Clamp,
            sampler::WrapMode::Clamp,
        ),
    };

    let sampler = ctx.sampler_create(&[sampler_create]).remove(0);

    Some((img, sampler))
}

pub unsafe fn image_color(
    ctx: &mut Context,
    submit: &mut SubmitGroup,
    content: &[u8],
    dimensions: (u32, u32),
    format: image::ImageFormat,
) -> Option<(image::ImageHandle, sampler::SamplerHandle)> {
    image_create_with_content(
        ctx,
        submit,
        content,
        dimensions,
        format,
        gfx::image::Usage::TRANSFER_DST | gfx::image::Usage::SAMPLED,
    )
}

pub unsafe fn image_color_empty(
    ctx: &mut Context,
    dimensions: (u32, u32),
    format: image::ImageFormat,
) -> Option<image::ImageHandle> {
    image_create(
        ctx,
        dimensions,
        format,
        gfx::image::Usage::TRANSFER_DST
            | gfx::image::Usage::TRANSFER_SRC
            | gfx::image::Usage::SAMPLED,
    )
}
