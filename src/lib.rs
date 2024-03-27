//! # mtzip
//!
//! MTZIP (Stands for Multi-Threaded ZIP) is a library for making zip archives while utilising all
//! available performance available with multithreading. The amount of threads can be limited by the
//! user or detected automatically.
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
//! zipper.add_file_from_fs("input/test_text_file.txt", "test_text_file.txt");
//!
//! // Adding a file from a byte array
//! zipper.add_file_from_memory(b"Hello, world!", "hello_world.txt");
//!
//! // Adding a directory and a file to it
//! zipper.add_directory("test_dir");
//! // And adding a file to it
//! zipper.add_file_from_fs("input/file_that_goes_to_a_dir.txt", "test_dir/file_that_goes_to_a_dir.txt");
//!
//! // Writing to a file
//! // First, open the file
//! let mut file = File::create("output.zip").unwrap();
//! // Then, write to it
//! zipper.write(&mut file); // Amount of threads is chosen automatically
//! ```

use std::{
    borrow::Cow,
    io::{Seek, Write},
    num::NonZeroUsize,
    path::Path,
    sync::{mpsc, Mutex},
};

use level::CompressionLevel;
use zip_archive_parts::{
    data::ZipData,
    job::{ZipJob, ZipJobOrigin},
};

pub mod level;
mod zip_archive_parts;

// TODO: Make another queue of jobs for simple records, such as directories
// TODO: Last mod datetime
// TODO: Add support for modification datetime using extra fields. Following is a list of PKWARE
// APPNOTE entries related to this:
//      - 4.3.12 Central directory structure
//      - 4.4.11 Extra field length
//      - 4.4.28 Extra field
//      - 4.5 Extensible data fields
//      - 4.5.5 NTFS Extra Field
//      - 4.5.7 UNIX Extra Field
// Useful form of the appnote in markdown: https://github.com/Majored/rs-async-zip/blob/main/SPECIFICATION.md

/// Compression type for the file. Directories always use [`Stored`](CompressionType::Stored).
/// Default is [`Deflate`](CompressionType::Deflate).
#[repr(u16)]
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum CompressionType {
    Stored = 0,
    #[default]
    Deflate = 8,
}

/// Initialize using [`Default`] trait implementation. Uses interior mutabillity for inner state
/// management (pending jobs and compressed data).
///
/// The lifetime `'d` indicates the lifetime of borrowed data supplied in
/// [`add_file_from_memory`](Self::add_file_from_memory).
///
/// The lifetime `'p` indicates the lifetime of borrowed [`Path`] supplied in
/// [`add_file_from_fs`](Self::add_file_from_fs).
#[derive(Debug, Default)]
pub struct ZipArchive<'d, 'p> {
    jobs_queue: Mutex<Vec<ZipJob<'d, 'p>>>,
    data: Mutex<ZipData>,
}

impl<'d, 'p> ZipArchive<'d, 'p> {
    /// Create an empty [`ZipArchive`]
    #[inline]
    pub fn new() -> Self {
        Self::default()
    }

    /// Add file from filesystem. Opens the file and reads data from it when
    /// [`compress`](Self::compress) is called.
    ///
    /// Default value for `compression_type` is [`Deflate`](CompressionType::Deflate).
    ///
    /// `compression_level` is ignored when [`CompressionType::Stored`] is used. Default value is
    /// [`CompressionLevel::best`].
    pub fn add_file_from_fs(
        &self,
        fs_path: impl Into<Cow<'p, Path>>,
        archived_path: String,
        compression_level: Option<CompressionLevel>,
        compression_type: Option<CompressionType>,
    ) {
        let name = archived_path;
        let job = ZipJob {
            data_origin: ZipJobOrigin::Filesystem {
                path: fs_path.into(),
                compression_level: compression_level.unwrap_or(CompressionLevel::best()),
                compression_type: compression_type.unwrap_or(CompressionType::Deflate),
            },
            archive_path: name,
        };
        {
            let mut jobs = self.jobs_queue.lock().unwrap();
            jobs.push(job);
        }
    }

    /// Add file from an owned data source. Data is stored in archive struct for later compression.
    /// Helps avoiding lifetime hell at the cost of allocation in some cases.
    ///
    /// Default value for `compression_type` is [`Deflate`](CompressionType::Deflate).
    ///
    /// `compression_level` is ignored when [`CompressionType::Stored`] is used. Default value is
    /// [`CompressionLevel::best`].
    pub fn add_file_from_memory(
        &self,
        data: impl Into<Cow<'d, [u8]>>,
        archived_path: String,
        compression_level: Option<CompressionLevel>,
        compression_type: Option<CompressionType>,
    ) {
        let job = ZipJob {
            data_origin: ZipJobOrigin::RawData {
                data: data.into(),
                compression_level: compression_level.unwrap_or(CompressionLevel::best()),
                compression_type: compression_type.unwrap_or(CompressionType::Deflate),
            },
            archive_path: archived_path,
        };
        {
            let mut jobs = self.jobs_queue.lock().unwrap();
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
            let mut jobs = self.jobs_queue.lock().unwrap();
            jobs.push(job);
        }
    }

    /// Compress contents. Will be done automatically on [`write`](Self::write) call if files were added
    /// between last compression and [`write`](Self::write) call. Automatically chooses amount of
    /// threads to use based on how much are available.
    #[inline]
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
        let jobs = &self.jobs_queue;
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
                    thread_tx.send(job.into_file().unwrap()).unwrap();
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
    #[inline]
    pub fn write<W: Write + Seek>(&self, writer: &mut W) -> std::io::Result<()> {
        self.write_with_threads(writer, Self::get_threads())
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
    pub fn write_with_threads<W: Write + Seek>(
        &self,
        writer: &mut W,
        threads: usize,
    ) -> std::io::Result<()> {
        if !self.jobs_queue.lock().unwrap().is_empty() {
            self.compress_with_threads(threads)
        }
        let data_lock = self.data.lock().unwrap();
        data_lock.write(writer)
    }

    fn get_threads() -> usize {
        std::thread::available_parallelism()
            .map(NonZeroUsize::get)
            .unwrap_or(1)
    }
}
