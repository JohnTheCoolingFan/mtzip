use std::io::{Seek, Write};

use super::extra_fields::ExtraFields;
use crate::CompressionType;

const VERSION_NEEDED_TO_EXTRACT: u16 = 20;
#[cfg(not(target_os = "windows"))]
const VERSION_MADE_BY: u16 = 0x033F;
#[cfg(target_os = "windows")]
const VERSION_MADE_BY: u16 = 0x0A3F;

const LOCAL_FILE_HEADER_SIGNATURE: u32 = 0x04034B50;
const CENTRAL_FILE_HEADER_SIGNATURE: u32 = 0x02014B50;
const GENERAL_PURPOSE_BIT_FLAG: u16 = 1 << 11;

#[derive(Debug)]
pub struct ZipFile {
    pub compression_type: CompressionType,
    pub crc: u32,
    pub uncompressed_size: u32,
    pub filename: String,
    pub data: Vec<u8>,
    pub external_file_attributes: u32,
    pub extra_fields: ExtraFields,
}

impl ZipFile {
    pub fn write_file_header_and_data<W: Write + Seek>(&self, buf: &mut W) -> std::io::Result<()> {
        // signature
        buf.write_all(&LOCAL_FILE_HEADER_SIGNATURE.to_le_bytes())?;
        // version needed to extract
        buf.write_all(&VERSION_NEEDED_TO_EXTRACT.to_le_bytes())?;
        // general purpose bit flag
        buf.write_all(&GENERAL_PURPOSE_BIT_FLAG.to_le_bytes())?;
        // compression type
        buf.write_all(&(self.compression_type as u16).to_le_bytes())?;
        // Last modification time // moved to extra fields
        buf.write_all(&0_u16.to_le_bytes())?;
        // Last modification date // moved to extra fields
        buf.write_all(&0_u16.to_le_bytes())?;
        // crc
        buf.write_all(&self.crc.to_le_bytes())?;
        // Compressed size
        debug_assert!(self.data.len() <= u32::MAX as usize);
        buf.write_all(&(self.data.len() as u32).to_le_bytes())?;
        // Uncompressed size
        buf.write_all(&self.uncompressed_size.to_le_bytes())?;
        // Filename size
        debug_assert!(self.filename.len() <= u16::MAX as usize);
        buf.write_all(&(self.filename.len() as u16).to_le_bytes())?;
        // extra field size
        buf.write_all(&self.extra_fields.data_length().to_le_bytes())?;

        // Filename
        buf.write_all(self.filename.as_bytes())?;
        // Extra field
        self.extra_fields.write(buf)?;

        // Data
        buf.write_all(&self.data)?;

        Ok(())
    }

    pub fn write_directory_entry<W: Write + Seek>(
        &self,
        buf: &mut W,
        local_header_offset: u32,
    ) -> std::io::Result<()> {
        // signature
        buf.write_all(&CENTRAL_FILE_HEADER_SIGNATURE.to_le_bytes())?;
        // version made by
        buf.write_all(&VERSION_MADE_BY.to_le_bytes())?;
        // version needed to extract
        buf.write_all(&VERSION_NEEDED_TO_EXTRACT.to_le_bytes())?;
        // general purpose bit flag
        buf.write_all(&GENERAL_PURPOSE_BIT_FLAG.to_le_bytes())?;
        // compression type
        buf.write_all(&(self.compression_type as u16).to_le_bytes())?;
        // Last modification time // moved to extra fields
        buf.write_all(&0_u16.to_le_bytes())?;
        // Last modification date // moved to extra fields
        buf.write_all(&0_u16.to_le_bytes())?;
        // crc
        buf.write_all(&self.crc.to_le_bytes())?;
        // Compressed size
        debug_assert!(self.data.len() <= u32::MAX as usize);
        buf.write_all(&(self.data.len() as u32).to_le_bytes())?;
        // Uncompressed size
        buf.write_all(&self.uncompressed_size.to_le_bytes())?;
        // Filename size
        debug_assert!(self.filename.len() <= u16::MAX as usize);
        buf.write_all(&(self.filename.len() as u16).to_le_bytes())?;
        // extra field size
        buf.write_all(&self.extra_fields.data_length().to_le_bytes())?;
        // comment size
        buf.write_all(&0_u16.to_le_bytes())?;
        // disk number start
        buf.write_all(&0_u16.to_le_bytes())?;
        // internal file attributes
        buf.write_all(&0_u16.to_le_bytes())?;
        // external file attributes
        buf.write_all(&self.external_file_attributes.to_le_bytes())?;
        // relative offset of local header
        buf.write_all(&local_header_offset.to_le_bytes())?;

        // Filename
        buf.write_all(self.filename.as_bytes())?;
        // Extra field
        self.extra_fields.write(buf)?;

        Ok(())
    }

    #[inline]
    pub fn directory(mut name: String, extra_fields: ExtraFields) -> Self {
        if !(name.ends_with('/') || name.ends_with('\\')) {
            name += "/"
        };
        Self {
            compression_type: CompressionType::Stored,
            crc: 0,
            uncompressed_size: 0,
            filename: name,
            data: vec![],
            external_file_attributes: 0o40755 << 16,
            extra_fields,
        }
    }
}
