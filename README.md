[![crates.io](https://img.shields.io/crates/v/mtzip?style=flat)](https://crates.io/crates/mtzip) [![crates.io](https://img.shields.io/crates/d/mtzip?style=flat)](https://crates.io/crates/mtzip)

# mtzip

MTZIP (Stands for Multi-Threaded ZIP) is a library for making zip archives
while utilising all available performance available with multithreading. The amount
of threads can be limited by the user or detected automatically.

Example usage:

```rs
use mtzip::ZipArchive;

// Creating the zipper that holds data and handles compression
let mut zipper = ZipArchive::new();

// Adding a file from filesystem
zipper.add_file_from_fs(
    Path::new("input/test_text_file.txt"),
    "test_text_file.txt".to_owned(),
);

// Adding a file with data from a memory location
zipper.add_file_from_memory(b"Hello, world!", "hello_world.txt".to_owned());

// Adding a directory and a file to it
zipper.add_directory("test_dir".to_owned());
zipper.add_file_from_fs(
    Path::new("input/file_that_goes_to_a_dir.txt"),
    "test_dir/file_that_goes_to_a_dir.txt".to_owned(),
);

// Writing to a file
// First, open the file
let mut file = File::create("output.zip").unwrap();
// Then, write to it
zipper.write(&mut file); // Amount of threads is chosen automatically
```

The amount of threads is also determined by the amount of files that are going to be compressed. Because Deflate compression cannot be multithreaded, the multithreading is achieved by having the files compressed individually. This means that if you have 12 threads available but only 6 files being added to the archive, you will only use 6 threads.

## Async

As each compression job runs in its own thread, there is no need to use async for concurrency between those. You can put the call to the `write` function into a separate blocking thread to do synchronous write I/O. Here is a list of helpers for using asynchronous I/O types as synchronous:

- `tokio`: [`SyncIoBridge` from `tokio-util`](https://docs.rs/tokio-util/latest/tokio_util/io/struct.SyncIoBridge.html)
- `futures`: [`Async{Read,Write}::compat_write`](https://docs.rs/futures/latest/futures/io/trait.AsyncWriteExt.html#method.compat_write). The `Compat` struct implements synchronous std I/O operations.

## Rayon

This crate also supports [`rayon`](https://crates.io/crates/rayon) for thread management and parallelism, enabled with `rayon` feature.

## Crate features

- `rust_backend` - enables `flate2/rust_backend` feature, enabled by default
- `zlib` - enables `flate2/zlib` feature
- `rayon` - enables rayon support
- `wasi_fs` - enabled use of WASI filesistem metadata extensions
