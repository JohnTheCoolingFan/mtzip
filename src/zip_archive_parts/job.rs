use std::{
    borrow::Cow,
    fs::{File, Metadata},
    io::Read,
    panic::{RefUnwindSafe, UnwindSafe},
    path::Path,
};

use cfg_if::cfg_if;
use derivative::Derivative;
use flate2::{read::DeflateEncoder, CrcReader};

use super::{extra_field::ExtraFields, file::ZipFile};
use crate::{level::CompressionLevel, zip_archive_parts::file::ZipFileHeader, CompressionType};

#[derive(Derivative)]
#[derivative(Debug)]
pub enum ZipJobData<'d, 'r> {
    RawData(Cow<'d, [u8]>),
    Reader(
        #[derivative(Debug = "ignore")]
        Box<dyn Read + Send + Sync + UnwindSafe + RefUnwindSafe + 'r>,
    ),
}

#[derive(Derivative)]
#[derivative(Debug)]
pub enum ZipJobOrigin<'d, 'p, 'r> {
    Filesystem {
        path: Cow<'p, Path>,
        compression_level: CompressionLevel,
        compression_type: CompressionType,
    },
    Directory {
        extra_fields: ExtraFields,
        external_attributes: u16,
    },
    Data {
        data: ZipJobData<'d, 'r>,
        compression_level: CompressionLevel,
        compression_type: CompressionType,
        extra_fields: ExtraFields,
        external_attributes: u16,
    },
}

#[derive(Debug)]
pub struct ZipJob<'a, 'p, 'r> {
    pub data_origin: ZipJobOrigin<'a, 'p, 'r>,
    pub archive_path: String,
    pub file_comment: Option<String>,
}

impl ZipJob<'_, '_, '_> {
    #[inline]
    const fn convert_attrs(attrs: u32) -> u16 {
        (attrs & 0xFFFF) as u16
    }

    #[inline]
    pub(crate) fn attributes_from_fs(metadata: &Metadata) -> u16 {
        cfg_if! {
            if #[cfg(target_os = "windows")] {
                use std::os::windows::fs::MetadataExt;
                Self::convert_attrs(metadata.file_attributes())
            } else if #[cfg(target_os = "linux")] {
                use std::os::linux::fs::MetadataExt;
                Self::convert_attrs(metadata.st_mode())
            } else if #[cfg(all(unix, not(target_os = "linux")))] {
                use std::os::unix::fs::MetadataExt;
                Self::convert_attrs(metadata.permissions().mode())
            } else {
                if metadata.is_dir() {
                    super::file::DEFAULT_UNIX_DIR_ATTRS
                } else {
                    super::file::DEFAULT_UNIX_FILE_ATTRS
                }
            }
        }
    }

    fn gen_file<R: Read>(
        source: R,
        uncompressed_size: Option<u32>,
        archive_path: String,
        attributes: u16,
        compression_level: CompressionLevel,
        compression_type: CompressionType,
        extra_fields: ExtraFields,
        file_comment: Option<String>,
    ) -> std::io::Result<ZipFile> {
        let mut crc_reader = CrcReader::new(source);
        let mut data = Vec::with_capacity(uncompressed_size.unwrap_or(0) as usize);
        let uncompressed_size = match compression_type {
            CompressionType::Deflate => {
                let mut encoder = DeflateEncoder::new(&mut crc_reader, compression_level.into());
                encoder.read_to_end(&mut data)?;
                encoder.total_in() as usize
            }
            CompressionType::Stored => crc_reader.read_to_end(&mut data)?,
        };
        debug_assert!(uncompressed_size <= u32::MAX as usize);
        let uncompressed_size = uncompressed_size as u32;
        data.shrink_to_fit();
        let crc = crc_reader.crc().sum();
        Ok(ZipFile {
            header: ZipFileHeader {
                compression_type: CompressionType::Deflate,
                crc,
                uncompressed_size,
                filename: archive_path,
                external_file_attributes: (attributes as u32) << 16,
                extra_fields,
                file_comment,
            },
            data,
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
                self.file_comment,
            )),
            ZipJobOrigin::Filesystem {
                path,
                compression_level,
                compression_type,
            } => {
                let file = File::open(path).unwrap();
                let file_metadata = file.metadata().unwrap();
                let uncompressed_size = file_metadata.len();
                debug_assert!(uncompressed_size <= u32::MAX.into());
                let uncompressed_size = uncompressed_size as u32;
                let external_file_attributes = Self::attributes_from_fs(&file_metadata);
                let extra_fields = ExtraFields::new_from_fs(&file_metadata);
                Self::gen_file(
                    file,
                    Some(uncompressed_size),
                    self.archive_path,
                    external_file_attributes,
                    compression_level,
                    compression_type,
                    extra_fields,
                    self.file_comment,
                )
            }
            ZipJobOrigin::Data {
                data: ZipJobData::RawData(data),
                compression_level,
                compression_type,
                extra_fields,
                external_attributes,
            } => {
                let uncompressed_size = data.len();
                debug_assert!(uncompressed_size <= u32::MAX as usize);
                let uncompressed_size = uncompressed_size as u32;
                Self::gen_file(
                    data.as_ref(),
                    Some(uncompressed_size),
                    self.archive_path,
                    external_attributes,
                    compression_level,
                    compression_type,
                    extra_fields,
                    self.file_comment,
                )
            }
            ZipJobOrigin::Data {
                data: ZipJobData::Reader(reader),
                compression_level,
                compression_type,
                extra_fields,
                external_attributes,
            } => Self::gen_file(
                reader,
                None,
                self.archive_path,
                external_attributes,
                compression_level,
                compression_type,
                extra_fields,
                self.file_comment,
            ),
        }
    }
}
