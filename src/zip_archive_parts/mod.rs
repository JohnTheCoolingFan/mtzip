use std::path::PathBuf;

pub mod data;
pub mod file;
pub mod job;

#[derive(Debug)]
pub enum ZipJobOrigin<'a> {
    Filesystem(PathBuf),
    RawData(&'a [u8]),
    RawDataOwned(Vec<u8>),
    Directory,
}
