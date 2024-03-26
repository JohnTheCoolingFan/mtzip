use std::path::PathBuf;

pub mod zip_data;
pub mod zip_file;
pub mod zip_job;

#[derive(Debug)]
pub enum ZipJobOrigin<'a> {
    Filesystem(PathBuf),
    RawData(&'a [u8]),
    RawDataOwned(Vec<u8>),
    Directory,
}
