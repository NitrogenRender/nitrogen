/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

//! A collection of useful types and function which don't quite fit somewhere else.

pub(crate) mod allocator;
pub(crate) mod pool;
pub mod storage;
pub mod submit_group;
pub(crate) mod transfer;

use std::borrow::Cow;

pub(crate) type CowString = Cow<'static, str>;
