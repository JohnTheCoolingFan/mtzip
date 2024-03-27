use std::{borrow::Cow, path::Path};

pub mod data;
pub mod file;
pub mod job;

#[derive(Debug)]
pub enum ZipJobOrigin<'d, 'p> {
    Filesystem(Cow<'p, Path>),
    RawData(Cow<'d, [u8]>),
    Directory,
}
