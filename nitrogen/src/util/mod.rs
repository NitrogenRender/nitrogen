/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

pub mod storage;
pub mod pool;
pub mod transfer;
pub mod submit_group;

use std::borrow::Cow;

pub type CowString = Cow<'static, str>;
