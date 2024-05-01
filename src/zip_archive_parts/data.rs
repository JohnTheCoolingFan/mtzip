use std::io::{Seek, Write};

use super::file::ZipFile;

const END_OF_CENTRAL_DIR_SIGNATURE: u32 = 0x06054B50;

#[derive(Debug, Default)]
pub struct ZipData {
    pub files: Vec<ZipFile>,
}

impl ZipData {
    const FOOTER_LENGTH: usize = 22;

    pub fn write<W: Write + Seek>(&self, buf: &mut W) -> std::io::Result<()> {
        let mut offsets: Vec<u32> = Vec::with_capacity(self.files.len());
        // Zip file records
        for file in &self.files {
            debug_assert!(buf.stream_position()? <= u32::MAX.into());
            offsets.push(buf.stream_position()? as u32);
            file.write_local_file_header_and_data(buf)?;
        }
        debug_assert!(buf.stream_position()? <= u32::MAX.into());
        let central_dir_offset = buf.stream_position()? as u32;
        // Zip directory entries
        for (file, offset) in self.files.iter().zip(offsets.iter()) {
            file.write_central_directory_entry(buf, *offset)?;
        }

        // End of central dir record
        debug_assert!(buf.stream_position()? <= u32::MAX.into());
        let central_dir_start = buf.stream_position()? as u32;

        // Temporary in-memory statically sized array
        let mut footer = [0; Self::FOOTER_LENGTH];
        {
            let mut footer_buf: &mut [u8] = &mut footer;

            // Signature
            footer_buf.write_all(&END_OF_CENTRAL_DIR_SIGNATURE.to_le_bytes())?;
            // number of this disk
            footer_buf.write_all(&0_u16.to_le_bytes())?;
            // number of the disk with start
            footer_buf.write_all(&0_u16.to_le_bytes())?;
            // Number of entries on this disk
            debug_assert!(self.files.len() <= u16::MAX as usize);
            footer_buf.write_all(&(self.files.len() as u16).to_le_bytes())?;
            // Number of entries
            footer_buf.write_all(&(self.files.len() as u16).to_le_bytes())?;
            // Central dir size
            footer_buf.write_all(&(central_dir_start - central_dir_offset).to_le_bytes())?;
            // Central dir offset
            footer_buf.write_all(&central_dir_offset.to_le_bytes())?;
            // Comment length
            footer_buf.write_all(&0_u16.to_le_bytes())?;
        }

        buf.write_all(&footer)?;

        Ok(())
    }
}
