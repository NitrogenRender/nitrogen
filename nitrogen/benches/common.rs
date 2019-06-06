/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

use std::cell::RefCell;
use std::rc::Rc;

pub struct BenchContext {
    pub ctx: nitrogen::Context,
    pub group: nitrogen::SubmitGroup,
}

impl BenchContext {
    pub fn new() -> Rc<RefCell<Self>> {
        let context = unsafe {
            let ctx = nitrogen::Context::new("bench", 1);
            let group = ctx.create_submit_group();

            BenchContext { ctx, group }
        };

        Rc::new(RefCell::new(context))
    }

    pub fn release(v: Rc<RefCell<Self>>) {
        let context = Rc::try_unwrap(v).ok().unwrap().into_inner();
        unsafe {
            let mut ctx = context.ctx;
            let submit = context.group;

            submit.release(&mut ctx);
            ctx.release();
        };
    }
}
