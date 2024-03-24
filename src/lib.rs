//! # mtzip
//!
//! MTZIP (Stands for Multi-Threaded ZIP) is a library for making zip archives while utilising all
//! available performance available with multithreading. Amount of threads can be limited by the
//! user.
//!
//! Example usage:
//!
//! ```ignore
//! use mtzip::ZipArchive;
//!
//! // Creating the zipper that holds data and handles compression
//! let zipper = ZipArchive::default();
//!
//! // Adding a file from filesystem
//! zipper.add_file("input/test_text_file.txt", "test_text_file.txt");
//!
//! // Adding a file from a byte array
//! zipper.add_file_from_slice(b"Hello, world!", "hello_world.txt");
//!
//! // Adding a directory and a file to it
//! zipper.add_directory("test_dir");
//! // And adding a file to it
//! zipper.add_file("input/file_that_goes_to_a_dir.txt", "test_dir/file_that_goes_to_a_dir.txt");
//!
//! // Writing to a file
//! // First, open the file
//! let mut file = File::create("output.zip").unwrap();
//! // Then, write to it
//! zipper.write(&mut file); // Amount of threads is chosen automatically
//! ```

#[cfg(target_os = "windows")]
use std::os::windows::fs::MetadataExt;
use std::{
    fs::File,
    io::{Read, Seek, Write},
    num::NonZeroUsize,
    path::PathBuf,
    sync::{mpsc, Mutex},
};

use cfg_if::cfg_if;
use flate2::{read::DeflateEncoder, Compression, CrcReader};

const VERSION_NEEDED_TO_EXTRACT: u16 = 20;
#[cfg(not(target_os = "windows"))]
const VERSION_MADE_BY: u16 = 0x033F;
#[cfg(target_os = "windows")]
const VERSION_MADE_BY: u16 = 0x0A3F;

const FILE_RECORD_SIGNATURE: u32 = 0x04034B50;
const DIRECTORY_ENTRY_SIGNATURE: u32 = 0x02014B50;
const END_OF_CENTRAL_DIR_SIGNATURE: u32 = 0x06054B50;

/// Making archives with stored compression is not supported yet and only used on directory
/// entries.
#[repr(u16)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompressionType {
    Stored = 0,
    Deflate = 8,
}

/// Initialize using [`Default`] trait implementation. Uses interior mutabillity for inner state
/// management (pending jobs and compressed data).
///
/// The lifetime indicates the lifetime of borrowed data supplied in
/// [`add_file_from_slice`](Self::add_file_from_slice).
#[derive(Debug, Default)]
pub struct ZipArchive<'a> {
    jobs: Mutex<Vec<ZipJob<'a>>>,
    data: Mutex<ZipData>,
}

impl<'a> ZipArchive<'a> {
    /// Add file from filesystem. Opens the file and reads data from it when
    /// [`compress`](Self::compress) is called.
    pub fn add_file(&self, fs_path: PathBuf, archived_path: impl ToString) {
        let name = archived_path.to_string();
        let job = ZipJob {
            data_origin: ZipJobOrigin::Filesystem(fs_path),
            archive_path: name,
        };
        {
            let mut jobs = self.jobs.lock().unwrap();
            jobs.push(job);
        }
    }

    /// Add file from slice. Data is stored in archive struct for later compression. May cause
    /// problems with lifetimes, as the reference must be valid throughout the whoel existence of
    /// [`Self`]. This can be avoided using
    /// [`add_file_from_owned_data`](Self::add_file_from_owned_data) instead.
    pub fn add_file_from_slice(&self, data: &'a [u8], archived_path: String) {
        let job = ZipJob {
            data_origin: ZipJobOrigin::RawData(data),
            archive_path: archived_path,
        };
        {
            let mut jobs = self.jobs.lock().unwrap();
            jobs.push(job);
        }
    }

    /// Add file from an owned data source. Data is stored in archive struct for later compression.
    /// Helps avoiding lifetime hell at the cost of allocation in some cases.
    pub fn add_file_from_owned_data(&self, data: Vec<u8>, archived_path: String) {
        let job = ZipJob {
            data_origin: ZipJobOrigin::RawDataOwned(data),
            archive_path: archived_path,
        };
        {
            let mut jobs = self.jobs.lock().unwrap();
            jobs.push(job);
        }
    }

    /// Add a directory entry. All directories in the tree should be added.
    pub fn add_directory(&self, archived_path: String) {
        let name = archived_path;
        let job = ZipJob {
            data_origin: ZipJobOrigin::Directory,
            archive_path: name,
        };
        {
            let mut jobs = self.jobs.lock().unwrap();
            jobs.push(job);
        }
    }

    /// Compress contents. Will be done automatically on [`write`](Self::write) call if files were added
    /// between last compression and [`write`](Self::write) call. Automatically chooses amount of
    /// threads to use based on how much are available.
    pub fn compress(&self) {
        self.compress_with_threads(Self::get_threads());
    }

    /// Compress contents. Will be done automatically on
    /// [`write_with_threads`](Self::write_with_threads) call if files were added between last
    /// compression and [`write`](Self::write). Allows specifying amount of threads that will be
    /// used.
    ///
    /// Example of getting amount of threads that this library uses in [`compress`](Self::compress):
    ///
    /// ```
    /// let threads = std::thread::available_parallelism().map(NonZeroUsize::get).unwrap_or(1);
    ///
    /// zipper.compress_with_threads(threads);
    /// ```
    pub fn compress_with_threads(&self, threads: usize) {
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

    /// Write compressed data to a writer (usually a file). Executes [`compress`](Self::compress)
    /// if files were added between last [`compress`](Self::compress) call and this call.
    /// Automatically chooses the amount of threads cpu has.
    pub fn write<W: Write + Seek>(&self, writer: &mut W) {
        self.write_with_threads(writer, Self::get_threads());
    }

    /// Write compressed data to a writer (usually a file). Executes
    /// [`compress_with_threads`](Self::compress_with_threads) if files were added between last
    /// [`compress`](Self::compress) call and this call. Allows specifying amount of threads that
    /// will be used.
    ///
    /// Example of getting amount of threads that this library uses in [`write`](Self::write):
    ///
    /// ```
    /// let threads = std::thread::available_parallelism().map(NonZeroUsize::get).unwrap_or(1);
    ///
    /// zipper.write_with_threads(threads);
    /// ```
    pub fn write_with_threads<W: Write + Seek>(&self, writer: &mut W, threads: usize) {
        if !self.jobs.lock().unwrap().is_empty() {
            self.compress_with_threads(threads)
        }
        let data_lock = self.data.lock().unwrap();
        data_lock.to_bytes(writer);
    }

    fn get_threads() -> usize {
        std::thread::available_parallelism()
            .map(NonZeroUsize::get)
            .unwrap_or(1)
    }
}

#[derive(Debug)]
struct ZipJob<'a> {
    data_origin: ZipJobOrigin<'a>,
    archive_path: String,
}

impl ZipJob<'_> {
    fn file_attributes(file: &File) -> std::io::Result<u32> {
        let metadata = file.metadata()?;
        cfg_if! {
            if #[cfg(target_os = "windows")] {
                Ok(metadata.file_attributes())
            } else if #[cfg(target_os = "linux")] {
                use std::os::linux::fs::MetadataExt;
                Ok(metadata.st_mode())
            } else if #[cfg(target_os = "unix")] {
                Ok(metadata.permissions().mode())
            } else {
                Ok(0o100644 << 16)
            }
        }
    }

    fn gen_file<R: Read>(
        source: R,
        uncompressed_size: u32,
        archive_path: String,
        attributes: Option<u32>,
    ) -> ZipFile {
        let crc_reader = CrcReader::new(source);
        let mut encoder = DeflateEncoder::new(crc_reader, Compression::new(9));
        let mut data = Vec::with_capacity(uncompressed_size as usize);
        encoder.read_to_end(&mut data).unwrap();
        data.shrink_to_fit();
        let crc_reader = encoder.into_inner();
        let crc = crc_reader.crc().sum();
        ZipFile {
            compression_type: CompressionType::Deflate,
            crc,
            uncompressed_size,
            filename: archive_path,
            data,
            external_file_attributes: attributes.unwrap_or(0),
        }
    }

    fn into_file(self) -> ZipFile {
        match self.data_origin {
            ZipJobOrigin::Directory => ZipFile::directory(self.archive_path),
            ZipJobOrigin::Filesystem(fs_path) => {
                let file = File::open(fs_path).unwrap();
                let file_metadata = file.metadata().unwrap();
                let uncompressed_size = file_metadata.len() as u32;
                let external_file_attributes = Self::file_attributes(&file).unwrap();
                Self::gen_file(
                    file,
                    uncompressed_size,
                    self.archive_path,
                    Some(external_file_attributes),
                )
            }
            ZipJobOrigin::RawData(in_data) => {
                let uncompressed_size = in_data.len() as u32;
                Self::gen_file(in_data, uncompressed_size, self.archive_path, None)
            }
            ZipJobOrigin::RawDataOwned(in_data) => {
                let uncompressed_size = in_data.len() as u32;
                Self::gen_file::<&[u8]>(
                    in_data.as_ref(),
                    uncompressed_size,
                    self.archive_path,
                    None,
                )
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
        // Time // TODO // Can only be done by adding chrono dependency
        buf.write_all(&0_u16.to_le_bytes()).unwrap();
        // Date // TODO // Can only be done by adding chrono dependency
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
        // Time // TODO // Can only be done by adding chrono dependency
        buf.write_all(&0_u16.to_le_bytes()).unwrap();
        // Date // TODO // Can only be done by adding chrono dependency
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
