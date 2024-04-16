//! # mtzip
//!
//! MTZIP (Stands for Multi-Threaded ZIP) is a library for making zip archives while utilising all
//! available performance available with multithreading. The amount of threads can be limited by
//! the user or detected automatically.
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
//! zipper.add_file_from_fs(
//!     "input/file_that_goes_to_a_dir.txt",
//!     "test_dir/file_that_goes_to_a_dir.txt",
//! );
//!
//! // Writing to a file
//! // First, open the file
//! let mut file = File::create("output.zip").unwrap();
//! // Then, write to it
//! zipper.write(&mut file); // Amount of threads is chosen automatically
//! ```

#![warn(missing_docs)]

use std::{
    borrow::Cow,
    io::{Read, Seek, Write},
    num::NonZeroUsize,
    path::Path,
    sync::{mpsc, Mutex},
};

use level::CompressionLevel;
use zip_archive_parts::{
    data::ZipData,
    extra_field::ExtraFields,
    file::ZipFile,
    job::{ZipJob, ZipJobOrigin},
};

pub mod level;
mod zip_archive_parts;

pub use zip_archive_parts::extra_field;

// TODO: tests, maybe examples

/// Compression type for the file. Directories always use [`Stored`](CompressionType::Stored).
/// Default is [`Deflate`](CompressionType::Deflate).
#[repr(u16)]
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum CompressionType {
    /// No compression at all, the data is stored as-is.
    ///
    /// This is used for directories because they have no data (no payload)
    Stored = 0,
    #[default]
    /// Deflate compression, the most common in ZIP files.
    Deflate = 8,
}

/// Structure that holds the current state of ZIP archive creation.
///
/// # Lifetimes
///
/// Because some of the methods allow supplying borrowed data, the lifetimes are used to indicate
/// that [`Self`](ZipArchive) borrows them. If you only provide owned data, such as
/// [`Vec<u8>`](Vec) or [`PathBuf`](std::path::PathBuf), you won't have to worry about lifetimes
/// and can simply use `'static`, if you ever need to specify them in your code.
///
/// - `'d` is the lifetime of borrowed data added via
/// [`add_file_from_memory`](Self::add_file_from_memory)
/// - `'p` is the lifetime of borrowed [`Path`]s used in
/// [`add_file_from_fs`](Self::add_file_from_fs)
/// - `'r` is the lifetime of of borrowed data in readers supplied to
/// [`add_file_from_reader`](Self::add_file_from_reader)
#[derive(Debug, Default)]
pub struct ZipArchive<'d, 'p, 'r> {
    jobs_queue: Vec<ZipJob<'d, 'p, 'r>>,
    data: ZipData,
}

impl<'d, 'p, 'r> ZipArchive<'d, 'p, 'r> {
    fn push_job(&mut self, job: ZipJob<'d, 'p, 'r>) {
        self.jobs_queue.push(job);
    }

    fn push_file(&mut self, file: ZipFile) {
        self.data.files.push(file);
    }

    /// Create an empty [`ZipArchive`]
    #[inline]
    pub fn new() -> Self {
        Self::default()
    }

    /// Add file from filesystem.
    ///
    /// Opens the file and reads data from it when [`compress`](Self::compress) is called.
    ///
    /// Default value for `compression_type` is [`Deflate`](CompressionType::Deflate).
    ///
    /// `compression_level` is ignored when [`CompressionType::Stored`] is used. Default value is
    /// [`CompressionLevel::best`].
    ///
    /// This method does not allow setting [`ExtraFields`] manually and instead uses the filesystem
    /// to obtain them.
    pub fn add_file_from_fs(
        &mut self,
        fs_path: impl Into<Cow<'p, Path>>,
        archived_path: String,
        compression_level: Option<CompressionLevel>,
        compression_type: Option<CompressionType>,
    ) {
        let job = ZipJob {
            data_origin: ZipJobOrigin::Filesystem {
                path: fs_path.into(),
                compression_level: compression_level.unwrap_or(CompressionLevel::best()),
                compression_type: compression_type.unwrap_or(CompressionType::Deflate),
            },
            archive_path: archived_path,
        };
        self.push_job(job);
    }

    /// Add file with data from memory.
    ///
    /// The data can be either borrowed or owned by the [`ZipArchive`] struct to avoid lifetime
    /// hell.
    ///
    /// Default value for `compression_type` is [`Deflate`](CompressionType::Deflate).
    ///
    /// `compression_level` is ignored when [`CompressionType::Stored`] is used. Default value is
    /// [`CompressionLevel::best`].
    ///
    /// `extra_fields` parameter allows setting extra attributes. Currently it supports NTFS and
    /// UNIX filesystem attributes, see more in [`ExtraFields`] description.
    pub fn add_file_from_memory(
        &mut self,
        data: impl Into<Cow<'d, [u8]>>,
        archived_path: String,
        compression_level: Option<CompressionLevel>,
        compression_type: Option<CompressionType>,
        file_attributes: Option<u16>,
        extra_fields: Option<ExtraFields>,
    ) {
        let job = ZipJob {
            data_origin: ZipJobOrigin::RawData {
                data: data.into(),
                compression_level: compression_level.unwrap_or(CompressionLevel::best()),
                compression_type: compression_type.unwrap_or(CompressionType::Deflate),
                external_attributes: file_attributes.unwrap_or(ZipFile::default_file_attrs()),
                extra_fields: extra_fields.unwrap_or_default(),
            },
            archive_path: archived_path,
        };
        self.push_job(job);
    }

    /// Add a file with data from a reader.
    ///
    /// This method takes any type implementing [`Read`] and allows it to have borrowed data (`'r`)
    ///
    /// Default value for `compression_type` is [`Deflate`](CompressionType::Deflate).
    ///
    /// `compression_level` is ignored when [`CompressionType::Stored`] is used. Default value is
    /// [`CompressionLevel::best`].
    ///
    /// `extra_fields` parameter allows setting extra attributes. Currently it supports NTFS and
    /// UNIX filesystem attributes, see more in [`ExtraFields`] description.
    pub fn add_file_from_reader<R: Read + Send + Sync + 'r>(
        &mut self,
        reader: R,
        archived_path: String,
        compression_level: Option<CompressionLevel>,
        compression_type: Option<CompressionType>,
        file_attributes: Option<u16>,
        extra_fields: Option<ExtraFields>,
    ) {
        let job = ZipJob {
            data_origin: ZipJobOrigin::Reader {
                reader: Box::new(reader),
                compression_level: compression_level.unwrap_or(CompressionLevel::best()),
                compression_type: compression_type.unwrap_or(CompressionType::Deflate),
                external_attributes: file_attributes.unwrap_or(ZipFile::default_file_attrs()),
                extra_fields: extra_fields.unwrap_or_default(),
            },
            archive_path: archived_path,
        };
        self.push_job(job)
    }

    /// Add a directory entry.
    ///
    /// All directories in the tree should be added. This method does not asssociate any filesystem
    /// properties to the entry.
    pub fn add_directory(&mut self, archived_path: String, attributes: Option<u16>) {
        let job = ZipJob {
            data_origin: ZipJobOrigin::Directory {
                extra_fields: ExtraFields::default(),
                external_attributes: attributes.unwrap_or(ZipFile::default_dir_attrs()),
            },
            archive_path: archived_path,
        };
        let file = job.into_file().expect("No failing code path");
        self.push_file(file);
    }

    /// Add a directory entry.
    ///
    /// All directories in the tree should be added. Use this method if you want to manually set
    /// filesystem properties of the directory.
    ///
    /// `extra_fields` parameter allows setting extra attributes. Currently it supports NTFS and
    /// UNIX filesystem attributes, see more in [`ExtraFields`] description.
    pub fn add_directory_with_metadata(
        &mut self,
        archived_path: String,
        extra_fields: ExtraFields,
        attributes: Option<u16>,
    ) {
        let job = ZipJob {
            data_origin: ZipJobOrigin::Directory {
                extra_fields,
                external_attributes: attributes.unwrap_or(ZipFile::default_dir_attrs()),
            },
            archive_path: archived_path,
        };
        let file = job.into_file().expect("No failing code path");
        self.push_file(file);
    }

    /// Add a directory entry.
    ///
    /// All directories in the tree should be added. This method will take the metadata from
    /// filesystem and add it to the entry in the zip file.
    pub fn add_directory_with_metadata_from_fs<P: AsRef<Path>>(
        &mut self,
        archived_path: String,
        fs_path: P,
    ) -> std::io::Result<()> {
        let metadata = std::fs::metadata(fs_path)?;
        let job = ZipJob {
            data_origin: ZipJobOrigin::Directory {
                extra_fields: ExtraFields::new_from_fs(&metadata),
                external_attributes: ZipJob::attributes_from_fs(&metadata),
            },
            archive_path: archived_path,
        };
        let file = job.into_file().expect("No failing code path");
        self.push_file(file);
        Ok(())
    }

    /// Compress contents. Will be done automatically on [`write`](Self::write) call if files were
    /// added between last compression and [`write`](Self::write) call. Automatically chooses
    /// amount of threads to use based on how much are available.
    #[inline]
    pub fn compress(&mut self) {
        self.compress_with_threads(Self::get_threads());
    }

    /// Compress contents. Will be done automatically on
    /// [`write_with_threads`](Self::write_with_threads) call if files were added between last
    /// compression and [`write`](Self::write). Allows specifying amount of threads that will be
    /// used.
    ///
    /// Example of getting amount of threads that this library uses in
    /// [`compress`](Self::compress):
    ///
    /// ```
    /// let threads = std::thread::available_parallelism()
    ///     .map(NonZeroUsize::get)
    ///     .unwrap_or(1);
    ///
    /// zipper.compress_with_threads(threads);
    /// ```
    pub fn compress_with_threads(&mut self, threads: usize) {
        let (tx, rx) = mpsc::channel();
        let jobs_drain = Mutex::new(self.jobs_queue.drain(..));
        let jobs_drain_ref = &jobs_drain;
        std::thread::scope(|s| {
            for _ in 0..threads {
                let thread_tx = tx.clone();
                s.spawn(move || {
                    while let Some(job) = { jobs_drain_ref.lock().unwrap().next() } {
                        thread_tx.send(job.into_file().unwrap()).unwrap();
                    }
                });
            }
        });
        drop(tx);
        self.data.files.extend(rx.iter());
    }

    /// Write compressed data to a writer (usually a file). Executes [`compress`](Self::compress)
    /// if files were added between last [`compress`](Self::compress) call and this call.
    /// Automatically chooses the amount of threads cpu has.
    #[inline]
    pub fn write<W: Write + Seek>(&mut self, writer: &mut W) -> std::io::Result<()> {
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
    /// let threads = std::thread::available_parallelism()
    ///     .map(NonZeroUsize::get)
    ///     .unwrap_or(1);
    ///
    /// zipper.compress_with_threads(threads);
    /// ```
    pub fn write_with_threads<W: Write + Seek>(
        &mut self,
        writer: &mut W,
        threads: usize,
    ) -> std::io::Result<()> {
        if !self.jobs_queue.is_empty() {
            self.compress_with_threads(threads)
        }
        self.data.write(writer)
    }

    fn get_threads() -> usize {
        std::thread::available_parallelism()
            .map(NonZeroUsize::get)
            .unwrap_or(1)
    }
}
