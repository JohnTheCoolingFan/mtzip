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

use std::{
    io::{Seek, Write},
    num::NonZeroUsize,
    path::PathBuf,
    sync::{mpsc, Mutex},
};

use zip_archive_parts::{data::ZipData, job::ZipJob, ZipJobOrigin};

pub mod level;
mod zip_archive_parts;

// TODO: Use io Results, propagate errors to caller
// TODO: Make another queue of jobs for simple records, such as directories
// TODO: Last mod datetime
// TODO: Allow setting compression level
// TODO: Add support for modification datetime using extra fields. Following is a list of PKWARE
// APPNOTE entries related to this:
//      - 4.3.12 Central directory structure
//      - 4.4.11 Extra field length
//      - 4.4.28 Extra field
//      - 4.5 Extensible data fields
//      - 4.5.5 NTFS Extra Field
//      - 4.5.7 UNIX Extra Field
// Useful form of the appnote in markdown: https://github.com/Majored/rs-async-zip/blob/main/SPECIFICATION.md

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
    jobs_queue: Mutex<Vec<ZipJob<'a>>>,
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
            let mut jobs = self.jobs_queue.lock().unwrap();
            jobs.push(job);
        }
    }

    /// Add file from slice. Data is stored in archive struct for later compression. May cause
    /// problems with lifetimes, as the reference must be valid throughout the whole existence of
    /// [`Self`]. This can be avoided using
    /// [`add_file_from_owned_data`](Self::add_file_from_owned_data) instead.
    pub fn add_file_from_slice(&self, data: &'a [u8], archived_path: String) {
        let job = ZipJob {
            data_origin: ZipJobOrigin::RawData(data),
            archive_path: archived_path,
        };
        {
            let mut jobs = self.jobs_queue.lock().unwrap();
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
        if !self.jobs_queue.lock().unwrap().is_empty() {
            self.compress_with_threads(threads)
        }
        let data_lock = self.data.lock().unwrap();
        data_lock.write(writer);
    }

    fn get_threads() -> usize {
        std::thread::available_parallelism()
            .map(NonZeroUsize::get)
            .unwrap_or(1)
    }
}
