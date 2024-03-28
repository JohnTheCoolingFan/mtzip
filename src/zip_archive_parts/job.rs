use std::{
    borrow::Cow,
    fs::{File, Metadata},
    io::Read,
    path::Path,
};

use cfg_if::cfg_if;
use flate2::{read::DeflateEncoder, CrcReader};

use super::{extra_field::ExtraFields, file::ZipFile};
use crate::{level::CompressionLevel, CompressionType};

#[derive(Debug)]
pub enum ZipJobOrigin<'d, 'p> {
    Filesystem {
        path: Cow<'p, Path>,
        compression_level: CompressionLevel,
        compression_type: CompressionType,
    },
    RawData {
        data: Cow<'d, [u8]>,
        compression_level: CompressionLevel,
        compression_type: CompressionType,
        extra_fields: ExtraFields,
        external_attributes: u16,
    },
    Directory {
        extra_fields: ExtraFields,
        external_attributes: u16,
    },
}

#[derive(Debug)]
pub struct ZipJob<'a, 'p> {
    pub data_origin: ZipJobOrigin<'a, 'p>,
    pub archive_path: String,
}

impl ZipJob<'_, '_> {
    #[inline]
    const fn convert_attrs(attrs: u32) -> u16 {
        (attrs & 0xFFFF) as u16
    }

    #[inline]
    pub(crate) fn attributes(metadata: &Metadata) -> u16 {
        cfg_if! {
            if #[cfg(target_os = "windows")] {
                use std::os::windows::fs::MetadataExt;
                Self::convert_attrs(metadata.file_attributes())
            } else if #[cfg(target_os = "linux")] {
                use std::os::linux::fs::MetadataExt;
                Self::convert_attrs(metadata.st_mode())
            } else if #[cfg(target_os = "unix")] {
                use std::os::unix::fs::MetadataExt;
                Self::convert_attrs(metadata.permissions().mode())
            } else {
                if metadata.is_dir() {
                    DEFAULT_UNIX_DIR_ATTRS
                } else {
                    DEFAULT_UNIX_FILE_ATTRS
                }
            }
        }
    }

    fn gen_file<R: Read>(
        source: R,
        uncompressed_size: u32,
        archive_path: String,
        attributes: u16,
        compression_level: CompressionLevel,
        compression_type: CompressionType,
        extra_fields: ExtraFields,
    ) -> std::io::Result<ZipFile> {
        let mut crc_reader = CrcReader::new(source);
        let mut data = Vec::with_capacity(uncompressed_size as usize);
        match compression_type {
            CompressionType::Deflate => {
                let mut encoder = DeflateEncoder::new(&mut crc_reader, compression_level.into());
                encoder.read_to_end(&mut data)?;
            }
            CompressionType::Stored => {
                crc_reader.read_to_end(&mut data)?;
            }
        }
        data.shrink_to_fit();
        let crc = crc_reader.crc().sum();
        Ok(ZipFile {
            compression_type: CompressionType::Deflate,
            crc,
            uncompressed_size,
            filename: archive_path,
            data,
            external_file_attributes: (attributes as u32) << 16,
            extra_fields,
        })
    }

    pub fn into_file(self) -> std::io::Result<ZipFile> {
        match self.data_origin {
            ZipJobOrigin::Directory {
                extra_fields,
                external_attributes,
            } => Ok(ZipFile::directory(
                self.archive_path,
                extra_fields,
                external_attributes,
            )),
            ZipJobOrigin::Filesystem {
                path,
                compression_level,
                compression_type,
            } => {
                let file = File::open(path).unwrap();
                let file_metadata = file.metadata().unwrap();
                debug_assert!(file_metadata.len() <= u32::MAX.into());
                let uncompressed_size = file_metadata.len() as u32;
                let external_file_attributes = Self::attributes(&file_metadata);
                let extra_fields = ExtraFields::new_from_fs(&file_metadata);
                Self::gen_file(
                    file,
                    uncompressed_size,
                    self.archive_path,
                    external_file_attributes,
                    compression_level,
                    compression_type,
                    extra_fields,
                )
            }
            ZipJobOrigin::RawData {
                data,
                compression_level,
                compression_type,
                extra_fields,
                external_attributes,
            } => {
                debug_assert!(data.len() <= u32::MAX as usize);
                let uncompressed_size = data.len() as u32;
                Self::gen_file(
                    data.as_ref(),
                    uncompressed_size,
                    self.archive_path,
                    external_attributes,
                    compression_level,
                    compression_type,
                    extra_fields,
                )
            }
        }
    }
}
