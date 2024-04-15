[![crates.io](https://img.shields.io/crates/v/mtzip?style=flat)](https://crates.io/crates/mtzip) [![crates.io](https://img.shields.io/crates/d/mtzip?style=flat)](https://crates.io/crates/mtzip)

# mtzip

MTZIP (Stands for Multi-Threaded ZIP) is a library for making zip archives
while utilising all available performance available with multithreading. The amount
of threads can be limited by the user or detected automatically.

Example usage:

```rs
use mtzip::ZipArchive;

// Creating the zipper that holds data and handles compression
let zipper = ZipArchive::default();

// Adding a file from filesystem
zipper.add_file_from_fs("input/test_text_file.txt", "test_text_file.txt");

// Adding a file from a byte array
zipper.add_file_from_memory(b"Hello, world!", "hello_world.txt");

// Adding a directory and a file to it
zipper.add_directory("test_dir");
// And adding a file to it
zipper.add_file_from_fs("input/file_that_goes_to_a_dir.txt", "test_dir/file_that_goes_to_a_dir.txt");

// Writing to a file
// First, open the file
let mut file = File::create("output.zip").unwrap();
// Then, write to it
zipper.write(&mut file); // Amount of threads is chosen automatically
```

The amount of threads is also determined by the amount of files that are going to be compressed. Because Deflate compression cannot be multithreaded, the multithreading is achieved by having the files compressed individually. This means that if you have 12 threads available but only 6 files being added to the archive, you will only use 6 threads.

## Rayon

This crate also supports [`rayon`](https://crates.io/crates/rayon) for thread management and parallelism, enabled with `rayon` feature.
