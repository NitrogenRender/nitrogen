pub mod storage;
pub mod transfer;

use std::borrow::Cow;

pub type CowString = Cow<'static, str>;