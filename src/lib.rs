//! mdict_svr library: read MDict (.mdx/.mdd) files, pack them into a
//! search-efficient mmap'd container (`qmd`), look words up, and serve them.

pub mod build;
pub mod error;
pub mod lookup;
pub mod md;
pub mod mdx;
pub mod qmd;
pub mod server;
pub mod web;

pub use error::{Error, Result};
