use std::{
    fs::{File, Metadata},
    io::Read,
};

use cfg_if::cfg_if;
use flate2::{read::DeflateEncoder, CrcReader};

use super::{file::ZipFile, ZipJobOrigin};
use crate::{level::CompressionLevel, CompressionType};

#[derive(Debug)]
pub struct ZipJob<'a, 'p> {
    pub data_origin: ZipJobOrigin<'a, 'p>,
    pub archive_path: String,
}

impl ZipJob<'_, '_> {
    #[inline]
    fn file_attributes(metadata: &Metadata) -> u32 {
        cfg_if! {
            if #[cfg(target_os = "windows")] {
                use std::os::windows::fs::MetadataExt;
                metadata.file_attributes()
            } else if #[cfg(target_os = "linux")] {
                use std::os::linux::fs::MetadataExt;
                metadata.st_mode()
            } else if #[cfg(target_os = "unix")] {
                metadata.permissions().mode()
            } else {
                0o100644 << 16
            }
        }
    }

    fn gen_file<R: Read>(
        source: R,
        uncompressed_size: u32,
        archive_path: String,
        attributes: Option<u32>,
        compression_level: CompressionLevel,
    ) -> std::io::Result<ZipFile> {
        let crc_reader = CrcReader::new(source);
        let mut encoder = DeflateEncoder::new(crc_reader, compression_level.into());
        let mut data = Vec::with_capacity(uncompressed_size as usize);
        encoder.read_to_end(&mut data)?;
        data.shrink_to_fit();
        let crc_reader = encoder.into_inner();
        let crc = crc_reader.crc().sum();
        Ok(ZipFile {
            compression_type: CompressionType::Deflate,
            crc,
            uncompressed_size,
            filename: archive_path,
            data,
            external_file_attributes: attributes.unwrap_or(0),
        })
    }

    pub fn into_file(self) -> std::io::Result<ZipFile> {
        match self.data_origin {
            ZipJobOrigin::Directory => Ok(ZipFile::directory(self.archive_path)),
            ZipJobOrigin::Filesystem {
                path,
                compression_level,
            } => {
                let file = File::open(path).unwrap();
                let file_metadata = file.metadata().unwrap();
                let uncompressed_size = file_metadata.len() as u32;
                let external_file_attributes = Self::file_attributes(&file_metadata);
                Self::gen_file(
                    file,
                    uncompressed_size,
                    self.archive_path,
                    Some(external_file_attributes),
                    compression_level,
                )
            }
            ZipJobOrigin::RawData {
                data,
                compression_level,
            } => {
                let uncompressed_size = data.len() as u32;
                Self::gen_file(
                    data.as_ref(),
                    uncompressed_size,
                    self.archive_path,
                    None,
                    compression_level,
                )
            }
        }
    }
}
