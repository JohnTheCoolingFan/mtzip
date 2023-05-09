use flate2::{read::DeflateEncoder, Compression, CrcReader};
use std::{
    cell::Cell,
    fs::File,
    io::{Read, Seek, Write},
    path::PathBuf,
    sync::{mpsc, Mutex},
};

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
    Deflate = 8,
}

/// Initialize using Default trait.
#[derive(Debug, Default)]
pub struct ZipArchive<'a> {
    jobs: Mutex<Vec<ZipJob<'a>>>,
    data: Mutex<ZipData>,
    compressed: Cell<bool>,
}

impl<'a> ZipArchive<'a> {
    /// Add file from filesystem. Will read on compression.
    pub fn add_file(&self, fs_path: impl Into<PathBuf>, archive_name: &str) {
        self.compressed.set(false);
        let name = archive_name.to_string();
        let job = ZipJob {
            data_origin: ZipJobOrigin::Filesystem(fs_path.into()),
            archive_path: name,
        };
        {
            let mut jobs = self.jobs.lock().unwrap();
            jobs.push(job);
        }
    }

    /// Add file from slice. Stores the data in archive struct for later compression.
    pub fn add_file_from_slice(&self, data: &'a [u8], archive_name: &str) {
        self.compressed.set(false);
        let data = data;
        let name = archive_name.to_string();
        let job = ZipJob {
            data_origin: ZipJobOrigin::RawData(data),
            archive_path: name,
        };
        {
            let mut jobs = self.jobs.lock().unwrap();
            jobs.push(job);
        }
    }

    /// Add file from an owned data source. Data is stored in archive struct for later compression.
    /// Helps avoiding lifetime hell at the cost of allocation in soem cases.
    pub fn add_file_from_owned_data(&self, data: impl Into<Vec<u8>>, archive_name: &str) {
        self.compressed.set(false);
        let name = archive_name.to_string();
        let job = ZipJob {
            data_origin: ZipJobOrigin::RawDataOwned(data.into()),
            archive_path: name,
        };
        {
            let mut jobs = self.jobs.lock().unwrap();
            jobs.push(job);
        }
    }

    /// Add a directory entry
    pub fn add_directory(&self, archive_name: &str) {
        self.compressed.set(false);
        let name = archive_name.to_string();
        let job = ZipJob {
            data_origin: ZipJobOrigin::Directory,
            archive_path: name,
        };
        {
            let mut jobs = self.jobs.lock().unwrap();
            jobs.push(job);
        }
    }

    /// Compress contents. Will be done automatically on [write] if files were added between last
    /// compression and [write]. Automatically chooses amount of threads cpu has.
    pub fn compress(&self) {
        let threads = std::thread::available_parallelism().unwrap().get();
        self.compress_with_threads(threads);
    }

    /// Compress contents. Will be done automatically on [write] if files were added between last
    /// compression and [write]. Allows specifying amount of threads that will be used.
    pub fn compress_with_threads(&self, threads: usize) {
        self.compressed.set(true);
        let (tx, rx) = mpsc::channel();
        let jobs = &self.jobs;
        std::thread::scope(|s| {
            for _ in 0..threads {
                let thread_tx = tx.clone();
                s.spawn(move || loop {
                    let job = {
                        let mut job_lock = jobs.lock().unwrap();
                        if job_lock.is_empty() {
                            break;
                        } else {
                            job_lock.pop().unwrap()
                        }
                    };
                    thread_tx.send(job.into_file()).unwrap();
                });
            }
        });
        drop(tx);
        {
            let mut data_lock = self.data.lock().unwrap();
            data_lock.files.extend(rx.iter());
        }
    }

    /// Write compressed data to a writer (usually a file). Executes [compress] if files were added
    /// between last [compress] call and this call. Automatically chooses the amount of threads cpu
    /// has.
    pub fn write<W: Write + Seek>(&self, writer: &mut W) {
        let threads = std::thread::available_parallelism().unwrap().get();
        self.write_with_threads(writer, threads);
    }

    /// Write compressed data to a writer (usually a file). Executes [compress_with_threads] if
    /// files were added between last [compress] call and this call. Allows specifying amount of
    /// threads that will be used.
    pub fn write_with_threads<W: Write + Seek>(&self, writer: &mut W, threads: usize) {
        if !self.compressed.get() {
            self.compress_with_threads(threads)
        }
        let data_lock = self.data.lock().unwrap();
        data_lock.to_bytes(writer);
    }
}

#[derive(Debug)]
struct ZipJob<'a> {
    data_origin: ZipJobOrigin<'a>,
    archive_path: String,
}

impl ZipJob<'_> {
    fn into_file(self) -> ZipFile {
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
                    external_file_attributes: 0o100644 << 16, // Possible improvement: read
                                                              // permissions/attributes from fs
                }
            }
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
                    external_file_attributes: 0o100644 << 16,
                }
            }
            ZipJobOrigin::RawDataOwned(in_data) => {
                let uncompressed_size = in_data.len() as u32;
                let crc_reader = CrcReader::<&[u8]>::new(in_data.as_ref());
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
                    external_file_attributes: 0o100644 << 16,
                }
            }
        }
    }
}

#[derive(Debug)]
enum ZipJobOrigin<'a> {
    Filesystem(PathBuf),
    RawData(&'a [u8]),
    RawDataOwned(Vec<u8>),
    Directory,
}

#[derive(Debug, Default)]
struct ZipData {
    files: Vec<ZipFile>,
}

impl ZipData {
    fn to_bytes<W: Write + Seek>(&self, buf: &mut W) {
        let mut offsets: Vec<u32> = Vec::new();
        // Zip file records
        for file in &self.files {
            offsets.push(buf.stream_position().unwrap() as u32);
            file.to_bytes_filerecord(buf);
        }
        let central_dir_offset = buf.stream_position().unwrap() as u32;
        // Zip directory entries
        for (file, offset) in self.files.iter().zip(offsets.iter()) {
            file.to_bytes_direntry(buf, *offset);
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

#[derive(Debug)]
struct ZipFile {
    compression_type: CompressionType,
    crc: u32,
    uncompressed_size: u32,
    filename: String,
    data: Vec<u8>,
    external_file_attributes: u32,
}

impl ZipFile {
    fn to_bytes_filerecord<W: Write + Seek>(&self, buf: &mut W) {
        // signature
        buf.write_all(&FILE_RECORD_SIGNATURE.to_le_bytes()).unwrap();
        // version needed to extract
        buf.write_all(&VERSION_NEEDED_TO_EXTRACT.to_le_bytes())
            .unwrap();
        // flags
        buf.write_all(&0_u16.to_le_bytes()).unwrap();
        // compression type
        buf.write_all(&(self.compression_type as u16).to_le_bytes())
            .unwrap();
        // Time // TODO
        buf.write_all(&0_u16.to_le_bytes()).unwrap();
        // Date // TODO
        buf.write_all(&0_u16.to_le_bytes()).unwrap();
        // crc
        buf.write_all(&self.crc.to_le_bytes()).unwrap();
        // Compressed size
        buf.write_all(&(self.data.len() as u32).to_le_bytes())
            .unwrap();
        // Uncompressed size
        buf.write_all(&self.uncompressed_size.to_le_bytes())
            .unwrap();
        // Filename size
        buf.write_all(&(self.filename.len() as u16).to_le_bytes())
            .unwrap();
        // extra field size
        buf.write_all(&0_u16.to_le_bytes()).unwrap();
        // Filename
        buf.write_all(self.filename.as_bytes()).unwrap();
        // Data
        buf.write_all(&self.data).unwrap();
    }

    fn to_bytes_direntry<W: Write + Seek>(&self, buf: &mut W, local_header_offset: u32) {
        // signature
        buf.write_all(&DIRECTORY_ENTRY_SIGNATURE.to_le_bytes())
            .unwrap();
        // version made by
        buf.write_all(&VERSION_MADE_BY.to_le_bytes()).unwrap();
        // version needed to extract
        buf.write_all(&VERSION_NEEDED_TO_EXTRACT.to_le_bytes())
            .unwrap();
        // flags
        buf.write_all(&0_u16.to_le_bytes()).unwrap();
        // compression type
        buf.write_all(&(self.compression_type as u16).to_le_bytes())
            .unwrap();
        // Time // TODO
        buf.write_all(&0_u16.to_le_bytes()).unwrap();
        // Date // TODO
        buf.write_all(&0_u16.to_le_bytes()).unwrap();
        // crc
        buf.write_all(&self.crc.to_le_bytes()).unwrap();
        // Compressed size
        buf.write_all(&(self.data.len() as u32).to_le_bytes())
            .unwrap();
        // Uncompressed size
        buf.write_all(&self.uncompressed_size.to_le_bytes())
            .unwrap();
        // Filename size
        buf.write_all(&(self.filename.len() as u16).to_le_bytes())
            .unwrap();
        // extra field size
        buf.write_all(&0_u16.to_le_bytes()).unwrap();
        // comment size
        buf.write_all(&0_u16.to_le_bytes()).unwrap();
        // disk number start
        buf.write_all(&0_u16.to_le_bytes()).unwrap();
        // internal file attributes
        buf.write_all(&0_u16.to_le_bytes()).unwrap();
        // external file attributes
        buf.write_all(&self.external_file_attributes.to_le_bytes())
            .unwrap();
        // relative offset of local header
        buf.write_all(&local_header_offset.to_le_bytes()).unwrap();
        // Filename
        buf.write_all(self.filename.as_bytes()).unwrap();
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
            external_file_attributes: 0o40755 << 16,
        }
    }
}
