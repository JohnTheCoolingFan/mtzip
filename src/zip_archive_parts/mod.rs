use std::{borrow::Cow, path::Path};

use crate::level::CompressionLevel;

pub mod data;
pub mod file;
pub mod job;

#[derive(Debug)]
pub enum ZipJobOrigin<'d, 'p> {
    Filesystem {
        path: Cow<'p, Path>,
        compression_level: CompressionLevel,
    },
    RawData {
        data: Cow<'d, [u8]>,
        compression_level: CompressionLevel,
    },
    Directory,
}
