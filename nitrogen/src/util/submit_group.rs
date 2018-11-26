/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

use gfx;
use gfx::Device;

use back;

use types::Semaphore;

use smallvec::SmallVec;

struct SubmitGroup {
    semaphores: Vec<Semaphore>,
    last_semaphores: SmallVec<[usize; 6]>,
}

impl SubmitGroup {

    pub fn new() -> Self {
        SubmitGroup {
            semaphores: Vec::new(),
            last_semaphores: SmallVec::new(),
        }
    }

    fn clear_prev_sems(&mut self) {
        self.last_semaphores.clear();
    }

    // FIXME this could be using an impl Trait existential type, but a bug in the compiler
    // prevents us from using it. https://github.com/rust-lang/rust/issues/53984
    pub fn last_semaphore_list<'a>(&'a self) -> Box<dyn Iterator<Item = (&'a Semaphore)> + 'a>
    {
        let iter = self.last_semaphores
            .as_slice()
            .iter()
            .map(move |i| {
                (&self.semaphores[*i])
            });

        // We could return iter here, but NOOOOOO the complainer is complaining
        Box::new(iter)
    }
}
