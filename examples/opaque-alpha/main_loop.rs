/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

use winit::{Event, EventsLoop, Window, WindowBuilder, WindowEvent};

use nitrogen::*;

pub struct CanvasSize(pub f32, pub f32);

pub trait UserData {
    fn graph(&self) -> graph::GraphHandle;

    fn output_image(&self) -> graph::ResourceName;
}

pub(crate) struct MainLoop<U: UserData> {
    events_loop: EventsLoop,
    _window: Window,

    ctx: Context,
    display: DisplayHandle,

    submits: Vec<submit_group::SubmitGroup>,
    submit_idx: usize,

    user_data: U,

    running: bool,
    size: (f32, f32),
}

impl<U: UserData> MainLoop<U> {
    pub fn new<F: FnOnce(&mut Context) -> U>(f: F) -> Self {
        let events_loop = EventsLoop::new();
        let window = WindowBuilder::new()
            .with_title("Nitrogen - Opaque-Alpha Demo")
            .build(&events_loop)
            .unwrap();

        let mut ctx = Context::new("opaque-alpha-demo", 1);
        let display = ctx.display_add(&window);

        let size = {
            let size = window.get_inner_size().unwrap();

            (size.width as f32, size.height as f32)
        };

        let submits = vec![ctx.create_submit_group(), ctx.create_submit_group()];

        let user_data = f(&mut ctx);

        MainLoop {
            events_loop,
            _window: window,

            ctx,
            display,

            running: true,
            size,

            user_data,

            submits,
            submit_idx: 0,
        }
    }

    pub fn running(&self) -> bool {
        self.running
    }

    pub fn iterate(&mut self, store: &mut graph::Store) {
        let submit = &mut self.submits[self.submit_idx];
        submit.wait(&mut self.ctx);

        // handle events and swapchain resizes
        {
            let mut close_requested = false;
            let mut new_size = None;
            self.events_loop.poll_events(|event| match event {
                Event::WindowEvent { event, .. } => match event {
                    WindowEvent::CloseRequested => {
                        close_requested = true;
                    }
                    WindowEvent::Resized(size) => {
                        new_size = Some((size.width as f32, size.height as f32));
                    }
                    _ => {}
                },
                _ => {}
            });

            self.running = !close_requested;

            if let Some(size) = new_size {
                // resize happened
                self.size = size;
                submit.display_setup_swapchain(&mut self.ctx, self.display);
            }
        }

        store.insert(CanvasSize(self.size.0, self.size.1));

        let context = graph::ExecutionContext {
            reference_size: (self.size.0 as u32, self.size.1 as u32),
        };

        // execute graph
        {
            let graph = self.user_data.graph();
            let image = self.user_data.output_image();

            if let Err(err) = self.ctx.graph_compile(graph) {
                println!("{:?}", err);
            }

            submit.graph_execute(&mut self.ctx, graph, store, &context);

            let image = self.ctx.graph_get_output_image(graph, image).unwrap();

            submit.display_present(&mut self.ctx, self.display, image);
        }

        self.submit_idx = (self.submit_idx + 1) % self.submits.len();
    }

    pub fn release(mut self) {
        self.submits[self.submit_idx].graph_destroy(&mut self.ctx, &[self.user_data.graph()]);

        for mut submit in self.submits {
            submit.wait(&mut self.ctx);
            submit.release(&mut self.ctx);
        }

        self.ctx.release();
    }
}
