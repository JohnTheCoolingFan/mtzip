use flate2::{read::DeflateEncoder, Compression, CrcReader};
use std::{sync::Mutex, path::Path, io::{Read, Write}, fs::File};

const VERSION_NEEDED_TO_EXTRACT: u16 = 20;
const VERSION_MADE_BY: u16 = 0x033F;

const FILE_RECORD_SIGNATURE: u32 = 0x04034B50;
const DIRECTORY_ENTRY_SIGNATURE: u32 = 0x02014B50;
const END_OF_CENTRAL_DIR_SIGNATURE: u32 = 0x06054B50;

/// Making archives with stored compression is not supported yet and only used on directory
/// entries.
#[repr(u16)]
#[derive(Debug, Clone, Copy)]
pub enum CompressionType {
    Stored = 0,
    Deflate = 8
}

/// Initialize using Default trait.
#[derive(Debug, Default)]
pub struct ZipArchive<'a> {
    jobs: Mutex<Vec<ZipJob<'a>>>,
    data: Mutex<ZipData>,
    compressed: Mutex<bool>
}

impl<'a> ZipArchive<'a> {
    /// Add file from silesystem. Will read on compression.
    pub fn add_file(&self, fs_path: &'a Path, archive_name: &str) {
        {
            let mut compressed = self.compressed.lock().unwrap();
            *compressed = false
        }
        let name = archive_name.to_string();
        let job = ZipJob{
            data_origin: ZipJobOrigin::Filesystem(fs_path),
            archive_path: name
        };
        {
            let mut jobs = self.jobs.lock().unwrap();
            jobs.push(job);
        }
    }

    /// Add file from slice. Stores the data in archive struct for later compression.
    pub fn add_file_from_slice(&self, data: &'a[u8], archive_name: &str) {
        {
            let mut compressed = self.compressed.lock().unwrap();
            *compressed = false
        }
        let data = data;
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

    /// Add a directory entry
    pub fn add_directory(&self, archive_name: &str) {
        {
            let mut compressed = self.compressed.lock().unwrap();
            *compressed = false
        }
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

    /// Call to execute compression. Will be done automatically on write if files were added before
    /// write.
    pub fn compress(&self, threads: usize) {
        {
            let mut compressed = self.compressed.lock().unwrap();
            *compressed = true
        }
        std::thread::scope(|s| {
            for _ in 0..threads {
                s.spawn(|| {
                    loop {
                        let job = {
                            let mut job_lock = self.jobs.lock().unwrap();
                            if job_lock.is_empty() {
                                break;
                            } else {
                                job_lock.pop().unwrap()
                            }
                        };
                        job.into_data(&self.data)
                    }
                });
            }
        })
    }

    /// Write compressed data to a writer. Automatically calls [compress](ZipArchive::compress) if files were added
    /// before write.
    pub fn write(&self, writer: &mut impl Write, threads: Option<usize>) {
        if !*self.compressed.lock().unwrap() {
            self.compress(threads.unwrap_or(1))
        }
        let data_lock = self.data.lock().unwrap();
        let mut data = Vec::with_capacity(data_lock.len());
        data_lock.to_bytes(&mut data);
        writer.write_all(&data).unwrap();
    }
}

#[derive(Debug)]
struct ZipJob<'a> {
    data_origin: ZipJobOrigin<'a>,
    archive_path: String
}

impl ZipJob<'_> {
    fn into_data(self, archive: &Mutex<ZipData>) {
        let data = {
            match self.data_origin {
                ZipJobOrigin::Directory => ZipFile::directory(self.archive_path),
                ZipJobOrigin::Filesystem(fs_path) => {
                    let file = File::open(fs_path).unwrap();
                    let uncompressed_size = file.metadata().unwrap().len() as u32;
                    let crc_reader = CrcReader::new(file);
                    let mut encoder = DeflateEncoder::new(crc_reader, Compression::new(9));
                    let mut data = Vec::new();
                    encoder.read_to_end(&mut data).unwrap();
                    let crc_reader = encoder.into_inner();
                    let crc = crc_reader.crc().sum();
                    ZipFile {
                        compression_type: CompressionType::Deflate,
                        crc,
                        uncompressed_size,
                        filename: self.archive_path,
                        data,
                        external_file_attributes: 0o100644 << 16 // Possible improvement: read
                                                                 // permissions/attributes from fs
                    }
                },
                ZipJobOrigin::RawData(in_data) => {
                    let uncompressed_size = in_data.len() as u32;
                    let crc_reader = CrcReader::new(in_data);
                    let mut encoder = DeflateEncoder::new(crc_reader, Compression::new(9));
                    let mut data = Vec::new();
                    encoder.read_to_end(&mut data).unwrap();
                    let crc_reader = encoder.into_inner();
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
enum ZipJobOrigin<'a> {
    Filesystem(&'a Path),
    RawData(&'a [u8]),
    Directory
}

#[derive(Debug, Default)]
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
        buf.extend(END_OF_CENTRAL_DIR_SIGNATURE.to_le_bytes());
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

    fn len(&self) -> usize {
        self.files.iter().fold(0, |total, file| total + file.len()) + 22
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
        buf.extend(FILE_RECORD_SIGNATURE.to_le_bytes());
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
        buf.extend(DIRECTORY_ENTRY_SIGNATURE.to_le_bytes());
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

    fn directory(mut name: String) -> Self {
        if !(name.ends_with('/') || name.ends_with('\\')) {
            name += "/"
        };
        Self {
            compression_type: CompressionType::Stored,
            crc: 0,
            uncompressed_size: 0,
            filename: name,
            data: vec![],
            external_file_attributes: 0o40755 << 16
        }
    }

    fn len(&self) -> usize {
        self.data.len() + self.filename.len() + self.filename.len() + 32 + 46
    }
}
