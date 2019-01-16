/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

use super::*;
use crate::graph::GraphResourcesResolved;

use gfx::buffer::Usage as BUsage;
use gfx::image::Usage as IUsage;

use crate::graph::{
    BufferReadType, BufferWriteType, ImageInfo, ImageReadType, ImageWriteType, ResourceCreateInfo,
    ResourceReadType, ResourceWriteType,
};

pub(crate) fn derive_resource_usage(
    backbuffer_usage: &BackbufferUsage,
    exec: &ExecutionGraph,
    resolved: &GraphResourcesResolved,
    outputs: &[ResourceId],
) -> ResourceUsages {
    let mut usages = ResourceUsages::default();

    for batch in &exec.pass_execution {
        derive_batch(backbuffer_usage, batch, resolved, &mut usages);
    }

    // outputs have to be readable somehow
    outputs
        .iter()
        .filter_map(|res| resolved.moved_from(*res))
        .for_each(|res| {
            usages.image.get_mut(&res).map(|(usage, _)| {
                *usage |= IUsage::SAMPLED;
                *usage |= IUsage::TRANSFER_SRC;
            });

            usages.buffer.get_mut(&res).map(|usage| {
                *usage |= BUsage::TRANSFER_SRC;
            });
        });

    usages
}

fn derive_batch(
    backbuffer_usage: &BackbufferUsage,
    batch: &ExecutionBatch,
    resolved: &GraphResourcesResolved,
    usages: &mut ResourceUsages,
) {
    // Start with info from creation
    for create in &batch.resource_create {
        let info = &resolved.infos[create];

        match info {
            ResourceCreateInfo::Buffer(_buf) => {
                let usage = BUsage::empty();

                usages.buffer.insert(*create, usage);
            }
            ResourceCreateInfo::Image(ImageInfo::Create(img)) => {
                let format = img.format.into();
                let usage = IUsage::empty();

                usages.image.insert(*create, (usage, format));
            }
            ResourceCreateInfo::Image(ImageInfo::BackbufferCreate(_, img, usage)) => {
                let format = img.format.into();
                let usage = *usage;

                usages.image.insert(*create, (usage, format));
            }
            ResourceCreateInfo::Image(ImageInfo::BackbufferRead(n)) => {
                // we don't really care about this, as all backbuffer resources have
                // explicit usages
                let format = backbuffer_usage.images[n].clone();
                usages
                    .image
                    .insert(*create, (gfx::image::Usage::empty(), format));
            }
            ResourceCreateInfo::Virtual => {
                // nothing to do here as we are not concerned with how external resources are
                // constructed
            }
        }
    }

    // See if a resource is used for copying
    for copy in &batch.resource_copies {
        let orig = resolved.copies_from[copy];

        // if this is an image
        if let Some((usage, format)) = usages.image.get(&orig).map(|x| x.clone()) {
            // if an image is created by copying another image, that means the src
            // has to be marked as TRANSFER_SRC and the new image as TRANSFER_DST
            let mut orig_usage = usage;

            orig_usage |= IUsage::TRANSFER_SRC;

            usages
                .image
                .get_mut(&orig)
                .map(move |entry| entry.0 = orig_usage);

            // once we copy we can get rid of all the previous flags, as they no longer apply
            let new_usage = IUsage::TRANSFER_DST;
            usages.image.insert(*copy, (new_usage, format));
        }

        // if this is a buffer
        if let Some(usage) = usages.buffer.get(&orig).map(|x| x.clone()) {
            // Same as for images, if we copy a buffer the src has to be TRANSFER_SRC and the new
            // buffer has to be TRANSFER_DST
            let mut orig_usage = usage;

            orig_usage |= BUsage::TRANSFER_SRC;

            usages
                .buffer
                .get_mut(&orig)
                .map(move |entry| *entry = orig_usage);

            // old flags don't apply to copies
            let new_usage = BUsage::TRANSFER_DST;
            usages.buffer.insert(*copy, new_usage);
        }
    }

    // Looking at all reads and writes in a pass
    for pass in &batch.passes {
        derive_pass(resolved, *pass, usages);
    }
}

fn derive_pass(
    resolved: &GraphResourcesResolved,
    pass: PassId,
    usages: &mut ResourceUsages,
) -> Option<()> {
    // inspect read types and adjust usage

    for (res, read_ty, _, _) in &resolved.pass_reads[&pass] {
        // TODO log this error and continue searching
        let origin = resolved.moved_from(*res)?;

        match read_ty {
            ResourceReadType::Buffer(buf) => {
                let mut usage = usages.buffer[&origin].clone();

                match buf {
                    BufferReadType::Storage => {
                        usage |= BUsage::STORAGE;
                    }
                    BufferReadType::StorageTexel => {
                        usage |= BUsage::STORAGE_TEXEL;
                    }
                    BufferReadType::Uniform => {
                        usage |= BUsage::UNIFORM;
                    }
                    BufferReadType::UniformTexel => {
                        usage |= BUsage::UNIFORM_TEXEL;
                    }
                }

                usages.buffer.insert(origin, usage);
            }
            ResourceReadType::Image(img) => {
                let (mut usage, format) = usages.image[&origin].clone();

                match img {
                    ImageReadType::Color => {
                        usage |= IUsage::SAMPLED;
                    }
                    ImageReadType::Storage => {
                        usage |= IUsage::STORAGE;
                    }
                    ImageReadType::DepthStencil => {
                        usage |= IUsage::DEPTH_STENCIL_ATTACHMENT;
                    }
                }

                usages.image.insert(origin, (usage, format));
            }
            ResourceReadType::Virtual => {
                // Nothing to do ...
            }
        }
    }

    // inspect write types and adjust usage

    for (res, write_ty, _) in &resolved.pass_writes[&pass] {
        let origin = resolved.moved_from(*res)?;

        match write_ty {
            ResourceWriteType::Buffer(buf) => {
                let mut usage = usages.buffer[&origin].clone();

                match buf {
                    BufferWriteType::Storage => {
                        usage |= BUsage::STORAGE;
                    }
                    BufferWriteType::StorageTexel => {
                        usage |= BUsage::STORAGE_TEXEL;
                    }
                }

                usages.buffer.insert(origin, usage);
            }
            ResourceWriteType::Image(img) => {
                let (mut usage, format) = match usages.image.get(&origin) {
                    Some(stuff) => stuff,
                    None => continue,
                };

                match img {
                    ImageWriteType::Color => {
                        usage |= IUsage::COLOR_ATTACHMENT;
                    }
                    ImageWriteType::DepthStencil => {
                        usage |= IUsage::DEPTH_STENCIL_ATTACHMENT;
                    }
                    ImageWriteType::Storage => {
                        usage |= IUsage::STORAGE;
                    }
                }

                usages.image.insert(origin, (usage, *format));
            }
        }
    }

    Some(())
}
