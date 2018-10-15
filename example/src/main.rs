extern crate winit;
extern crate nitrogen;

fn main() {

    let mut events = winit::EventsLoop::new();
    let window = winit::Window::new(&events).unwrap();

    let mut ntg = {
        let create_info = nitrogen::CreationInfo {
            name: "nitrogen example".into(),
            version: 1,
            window: &window,
        };
        nitrogen::Context::setup_winit(create_info)
    };

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

            ntg.display_ctx.setup_swapchain(&ntg.device_ctx);

            resized = false;
        }

        ntg.display_ctx.present(&ntg.device_ctx);

    }

    ntg.release();
}
