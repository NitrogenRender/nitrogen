extern crate winit;
extern crate nitrogen;
extern crate image;

#[macro_use]
extern crate log;

fn main() {

    let mut events = winit::EventsLoop::new();
    let window = winit::Window::new(&events).unwrap();

    let mut ntg = nitrogen::Context::new("nitrogen test", 1);

    let display = ntg.add_display(&window);

    let (image, sampler) = {

        let image_data = include_bytes!("../test.png");

        let image = image::load(std::io::Cursor::new(&image_data[..]), image::PNG)
            .unwrap()
            .to_rgba();

        let (width, height) = image.dimensions();
        let dimension = nitrogen::image::ImageDimension::D2 { x: width, y: height };

        let create_info = nitrogen::image::ImageCreateInfo {
            dimension,
            num_layers: 1,
            num_samples: 1,
            num_mipmaps: 1,

            used_as_transfer_dst: true,
            used_for_sampling: true,
            .. Default::default()
        };

        let img = ntg.image_storage.create(&ntg.device_ctx, create_info).unwrap();


        debug!("width {}, height {}", width, height);

        {
            let data = nitrogen::image::ImageUploadInfo {
                data: &(*image),
                format: nitrogen::image::ImageFormat::RgbaUnorm,
                dimension,
                target_offset: (0, 0, 0),
            };

            ntg.image_storage.upload_data(&ntg.device_ctx, img, data).unwrap();
        }

        drop(image);

        let sampler = {

            use nitrogen::sampler::{Filter, WrapMode};

            let sampler_create = nitrogen::sampler::SamplerCreateInfo {
                min_filter: Filter::Linear,
                mag_filter: Filter::Linear,
                mip_filter: Filter::Linear,
                wrap_mode: (WrapMode::Clamp, WrapMode::Clamp, WrapMode::Clamp),
            };

            ntg.sampler_storage.create(&ntg.device_ctx, sampler_create)
        };

        (img, sampler)
    };

    ntg.displays[display].setup_swapchain(&ntg.device_ctx);

    let mut running = true;
    let mut resized = true;

    while running {

        events.poll_events(|event| {
            match event {
                winit::Event::WindowEvent { event, .. } => {
                    match event {
                        winit::WindowEvent::CloseRequested => {
                            running = false;
                        },
                        winit::WindowEvent::Resized(_size) => {
                            resized = true;
                        },
                        _ => {
                        }
                    }
                },
                _ => {
                }
            }
        });


        if resized {

            debug!("resize!");

            ntg.displays[display].setup_swapchain(&ntg.device_ctx);

            resized = false;
        }

        ntg.displays[display].present(
            &ntg.device_ctx,
            &ntg.image_storage,
            image,
            &ntg.sampler_storage,
            sampler
        );
    }

    ntg.sampler_storage.destroy(&ntg.device_ctx, sampler);
    ntg.image_storage.destroy(&ntg.device_ctx, image);

    ntg.release();


}
