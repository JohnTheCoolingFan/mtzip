use flate2::{read::DeflateEncoder, Compression, CrcReader};
use std::{sync::Mutex, path::Path, io::{Read, Write}, fs::File};

const VERSION_NEEDED_TO_EXTRACT: u16 = 20;
const VERSION_MADE_BY: u16 = 814;

const FILE_RECORD_SIGNATURE: &[u8; 4] = b"PK34";
const DIRECTORY_ENTRY_SIGNATURE: &[u8; 4] = b"PK12";
const END_OF_CENTRAL_DIR_SIGNATURE: &[u8; 4] = b"PK56";

#[repr(u16)]
#[derive(Debug, Clone, Copy)]
pub enum CompressionType {
    Stored = 0,
    Deflate = 8
}

#[derive(Debug)]
pub struct ZipArchive {
    jobs: Mutex<Vec<ZipJob>>,
    data: Mutex<ZipData>
}

impl ZipArchive {
    pub fn add_file(&self, fs_path: impl AsRef<Path>, archive_name: &str) {
        let path: Box<Path> = fs_path.as_ref().into();
        let name = archive_name.to_string();
        let job = ZipJob{
            data_origin: ZipJobOrigin::Filesystem(path),
            archive_path: name
        };
        {
            let mut jobs = self.jobs.lock().unwrap();
            jobs.push(job);
        }
    }

    pub fn add_file_from_slice(&self, data: &[u8], archive_name: &str) {
        let data = Vec::from(data);
        let name = archive_name.to_string();
        let job = ZipJob {
            data_origin: ZipJobOrigin::RawData(data),
            archive_path: name
        };
        {
            let mut jobs = self.jobs.lock().unwrap();
            jobs.push(job);
        }
    }

    pub fn add_directory(&self, archive_name: &str) {
        let name = archive_name.to_string();
        let job = ZipJob {
            data_origin: ZipJobOrigin::Directory,
            archive_path: name
        };
        {
            let mut jobs = self.jobs.lock().unwrap();
            jobs.push(job);
        }
    }

    pub fn compress(&self) { // TODO: make this multithreaded
        let mut jobs_lock = self.jobs.lock().unwrap();
        for job in jobs_lock.drain(0..) {
            job.to_data(&self.data);
        }
    }

    pub fn write(&self, writer: &mut impl Write) {
        let data_lock = self.data.lock().unwrap();
        let mut data = Vec::new();
        data_lock.to_bytes(&mut data);
        writer.write_all(&data).unwrap();
    }
}

#[derive(Debug)]
struct ZipJob {
    data_origin: ZipJobOrigin,
    archive_path: String
}

impl ZipJob {
    fn to_data(self, archive: &Mutex<ZipData>) {
        let data = {
            match self.data_origin {
                ZipJobOrigin::Directory => ZipFile::directory(self.archive_path),
                ZipJobOrigin::Filesystem(fs_path) => {
                    let file = File::open(fs_path).unwrap();
                    let uncompressed_size = file.metadata().unwrap().len() as u32;
                    let mut encoder = DeflateEncoder::new(file, Compression::new(9));
                    let mut data = Vec::new();
                    encoder.read_to_end(&mut data).unwrap();
                    let file = encoder.into_inner();
                    let crc_reader = CrcReader::new(file);
                    let crc = crc_reader.crc().sum();
                    ZipFile {
                        compression_type: CompressionType::Deflate,
                        crc,
                        uncompressed_size,
                        filename: self.archive_path,
                        data,
                        external_file_attributes: 0o100644 << 16
                    }
                },
                ZipJobOrigin::RawData(in_data) => {
                    let uncompressed_size = in_data.len() as u32;
                    let mut encoder = DeflateEncoder::new(in_data.as_slice(), Compression::new(9));
                    let mut data = Vec::new();
                    encoder.read_to_end(&mut data).unwrap();
                    let in_data = encoder.into_inner();
                    let crc_reader = CrcReader::new(in_data);
                    let crc = crc_reader.crc().sum();
                    ZipFile {
                        compression_type: CompressionType::Deflate,
                        crc,
                        uncompressed_size,
                        filename: self.archive_path,
                        data,
                        external_file_attributes: 0o100644 << 16
                    }
                }
            }
        };
        {
            let mut data_lock = archive.lock().unwrap();
            data_lock.files.push(data);
        }
    }
}

#[derive(Debug)]
enum ZipJobOrigin {
    Filesystem(Box<Path>),
    RawData(Vec<u8>),
    Directory
}

#[derive(Debug)]
struct ZipData {
    files: Vec<ZipFile>
}

impl ZipData {
    fn to_bytes(&self, buf: &mut Vec<u8>) {
        let mut offsets: Vec<u32> = Vec::new();
        // Zip file records
        for file in &self.files {
            offsets.push(buf.len() as u32);
            file.to_bytes_filerecord(buf);
        }
        let central_dir_offset = buf.len() as u32;
        // Zip directory entries
        for (file, offset) in self.files.iter().zip(offsets.iter()) {
            file.to_bytes_direntry(buf, *offset);
        }

        // End of central dir record
        let central_dir_start = buf.len() as u32;

        // Signature
        buf.extend(END_OF_CENTRAL_DIR_SIGNATURE);
        // number of this disk
        buf.extend(0_u16.to_le_bytes());
        // number of the disk with start
        buf.extend(0_u16.to_le_bytes());
        // Number of entries on this disk
        buf.extend((self.files.len() as u16).to_le_bytes());
        // Number of entries
        buf.extend((self.files.len() as u16).to_le_bytes());
        // Central dir size
        buf.extend((central_dir_start - central_dir_offset).to_le_bytes());
        // Central dir offset
        buf.extend(central_dir_offset.to_le_bytes());
        // Comment length
        buf.extend(0_u16.to_le_bytes());
    }
}

#[derive(Debug)]
struct ZipFile {
    compression_type: CompressionType,
    crc: u32,
    uncompressed_size: u32,
    filename: String,
    data: Vec<u8>,
    external_file_attributes: u32
}

impl ZipFile {
    fn to_bytes_filerecord(&self, buf: &mut Vec<u8>) {
        // signature
        buf.extend(FILE_RECORD_SIGNATURE);
        // version needed to extract
        buf.extend(VERSION_NEEDED_TO_EXTRACT.to_le_bytes()); 
        // flags
        buf.extend(0_u16.to_le_bytes());
        // compression type
        buf.extend((self.compression_type as u16).to_le_bytes());
        // Time // TODO
        buf.extend(0_u16.to_le_bytes());
        // Date // TODO
        buf.extend(0_u16.to_le_bytes());
        // crc 
        buf.extend(self.crc.to_le_bytes());
        // Compressed size
        buf.extend((self.data.len() as u32).to_le_bytes());
        // Uncompressed size
        buf.extend(self.uncompressed_size.to_le_bytes());
        // Filename size
        buf.extend((self.filename.len() as u16).to_le_bytes());
        // extra field size
        buf.extend(0_u16.to_le_bytes());
        // Filename
        buf.extend(self.filename.as_bytes());
        // Data
        buf.extend(&self.data);
    }

    fn to_bytes_direntry(&self, buf: &mut Vec<u8>, local_header_offset: u32) {
        // signature
        buf.extend(DIRECTORY_ENTRY_SIGNATURE);
        // version made by
        buf.extend(VERSION_MADE_BY.to_le_bytes());
        // version needed to extract
        buf.extend(VERSION_NEEDED_TO_EXTRACT.to_le_bytes());
        // flags
        buf.extend(0_u16.to_le_bytes());
        // compression type
        buf.extend((self.compression_type as u16).to_le_bytes());
        // Time // TODO
        buf.extend(0_u16.to_le_bytes());
        // Date // TODO
        buf.extend(0_u16.to_le_bytes());
        // crc
        buf.extend(self.crc.to_le_bytes());
        // Compressed size
        buf.extend((self.data.len() as u32).to_le_bytes());
        // Uncompressed size
        buf.extend(self.uncompressed_size.to_le_bytes());
        // Filename size
        buf.extend((self.filename.len() as u16).to_le_bytes());
        // extra field size
        buf.extend(0_u16.to_le_bytes());
        // comment size
        buf.extend(0_u16.to_le_bytes());
        // disk number start
        buf.extend(0_u16.to_le_bytes());
        // internal file attributes
        buf.extend(0_u16.to_le_bytes());
        // external file attributes
        buf.extend(self.external_file_attributes.to_le_bytes());
        // relative offset of local header
        buf.extend(local_header_offset.to_le_bytes());
        // Filename
        buf.extend(self.filename.as_bytes());
    }

    fn directory(name: String) -> Self {
        Self {
            compression_type: CompressionType::Stored,
            crc: 0,
            uncompressed_size: 0,
            filename: name,
            data: vec![],
            external_file_attributes: 0o40755 << 16 // TODO
        }
    }
}
