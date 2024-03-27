use std::{borrow::Cow, path::PathBuf};

pub mod data;
pub mod file;
pub mod job;

#[derive(Debug)]
pub enum ZipJobOrigin<'a> {
    Filesystem(PathBuf),
    RawData(Cow<'a, [u8]>),
    Directory,
}
