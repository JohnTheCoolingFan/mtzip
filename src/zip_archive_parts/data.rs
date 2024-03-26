use std::io::{Seek, Write};

use super::file::ZipFile;

const END_OF_CENTRAL_DIR_SIGNATURE: u32 = 0x06054B50;

#[derive(Debug, Default)]
pub struct ZipData {
    pub files: Vec<ZipFile>,
}

impl ZipData {
    pub fn write<W: Write + Seek>(&self, buf: &mut W) {
        let mut offsets: Vec<u32> = Vec::with_capacity(self.files.len());
        // Zip file records
        for file in &self.files {
            offsets.push(buf.stream_position().unwrap() as u32);
            file.write_file_header_and_data(buf);
        }
        let central_dir_offset = buf.stream_position().unwrap() as u32;
        // Zip directory entries
        for (file, offset) in self.files.iter().zip(offsets.iter()) {
            file.write_directory_entry(buf, *offset);
        }

        // End of central dir record
        let central_dir_start = buf.stream_position().unwrap() as u32;

        // Signature
        buf.write_all(&END_OF_CENTRAL_DIR_SIGNATURE.to_le_bytes())
            .unwrap();
        // number of this disk
        buf.write_all(&0_u16.to_le_bytes()).unwrap();
        // number of the disk with start
        buf.write_all(&0_u16.to_le_bytes()).unwrap();
        // Number of entries on this disk
        buf.write_all(&(self.files.len() as u16).to_le_bytes())
            .unwrap();
        // Number of entries
        buf.write_all(&(self.files.len() as u16).to_le_bytes())
            .unwrap();
        // Central dir size
        buf.write_all(&(central_dir_start - central_dir_offset).to_le_bytes())
            .unwrap();
        // Central dir offset
        buf.write_all(&central_dir_offset.to_le_bytes()).unwrap();
        // Comment length
        buf.write_all(&0_u16.to_le_bytes()).unwrap();
    }
}
