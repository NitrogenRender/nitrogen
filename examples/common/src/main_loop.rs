/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

use winit::{Event, EventsLoop, Window, WindowBuilder, WindowEvent};

use nitrogen::graph::Store;
use nitrogen::submit_group::SubmitGroup;
use nitrogen::*;

use std::time::Instant;

pub struct CanvasSize(pub f32, pub f32);

pub trait UserData: Sized {
    fn iteration(&mut self, _store: &mut graph::Store, _delta: f64) {}

    fn graph(&self) -> graph::GraphHandle;

    fn output_image(&self) -> graph::ResourceName;

    fn release(self, ctx: &mut Context, submit: &mut SubmitGroup) {
        submit.graph_destroy(ctx, &[self.graph()]);
    }
}

pub struct MainLoop<U: UserData> {
    events_loop: EventsLoop,
    _window: Window,

    ctx: Context,
    display: DisplayHandle,

    submits: Vec<submit_group::SubmitGroup>,
    submit_idx: usize,

    user_data: U,
    store: Store,

    last_iter: Instant,

    running: bool,
    size: (f32, f32),
}

impl<U: UserData> MainLoop<U> {
    pub unsafe fn new<F>(name: &str, f: F) -> Option<Self>
    where
        F: FnOnce(&mut Store, &mut Context, &mut SubmitGroup) -> Option<U>,
    {
        let events_loop = EventsLoop::new();
        let window = WindowBuilder::new()
            .with_title(name)
            .build(&events_loop)
            .ok()?;

        let mut ctx = Context::new(name, 1);
        let display = ctx.display_add(&window);

        let size = {
            let size = window.get_inner_size().unwrap();

            (size.width as f32, size.height as f32)
        };

        let mut store = Store::new();

        store.insert(CanvasSize(size.0, size.1));

        let mut submits = vec![
            ctx.create_submit_group(),
            ctx.create_submit_group(),
            ctx.create_submit_group(),
            ctx.create_submit_group(),
        ];

        let user_data = match f(&mut store, &mut ctx, &mut submits[0]) {
            Some(d) => d,
            None => {
                for s in submits {
                    s.release(&mut ctx);
                }

                ctx.release();
                return None;
            }
        };

        let instant = Instant::now();

        Some(MainLoop {
            events_loop,
            _window: window,

            ctx,
            display,

            running: true,
            size,

            user_data,
            store,

            last_iter: instant,

            submits,
            submit_idx: 0,
        })
    }

    pub fn running(&self) -> bool {
        self.running
    }

    pub unsafe fn iterate(&mut self) {
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

        self.store.insert(CanvasSize(self.size.0, self.size.1));

        let delta = {
            let new_instant = Instant::now();
            let dur = new_instant.duration_since(self.last_iter);
            self.last_iter = new_instant;

            const NANOS_PER_SEC: u32 = 1_000_000_000;

            let secs = dur.as_secs() as f64;
            let subsecs = dur.subsec_nanos() as f64 / NANOS_PER_SEC as f64;

            secs + subsecs
        };

        self.user_data.iteration(&mut self.store, delta);

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

            submit.graph_execute(&mut self.ctx, graph, &mut self.store, &context);

            let image = submit.graph_get_image(&self.ctx, graph, image).unwrap();

            submit.display_present(&mut self.ctx, self.display, image);
        }

        self.submit_idx = (self.submit_idx + 1) % self.submits.len();
    }

    pub unsafe fn release(mut self) {
        for submit_group in &mut self.submits {
            submit_group.wait(&mut self.ctx);
        }

        self.ctx.wait_idle();

        self.user_data
            .release(&mut self.ctx, &mut self.submits[self.submit_idx]);

        self.ctx.wait_idle();

        for mut submit in self.submits {
            submit.wait(&mut self.ctx);
            submit.release(&mut self.ctx);
        }

        self.ctx.release();
    }
}
