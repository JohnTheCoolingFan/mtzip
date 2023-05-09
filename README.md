# mtzip

MTZIP (Stands for Multi-Threaded ZIP) is a library for making zip archives
while utilising all available performance available with multithreading. Amount
of threads can be limited by the user.

Example usage:

```rs
use mtzip::ZipArchive;

// Creating the zipper that holds data and handles compression
let zipper = ZipArchive::default();

// Adding a file from filesystem
zipper.add_file("input/test_text_file.txt", "test_text_file.txt");

// Adding a file from a byte array
zipper.add_file_from_slice(b"Hello, world!", "hello_world.txt");

// Adding a directory and a file to it
zipper.add_directory("test_dir");
// And adding a file to it
zipper.add_file("input/file_that_goes_to_a_dir.txt", "test_dir/file_that_goes_to_a_dir.txt");

// Writing to a file
// First, open the file
let mut file = File::create("output.zip").unwrap();
// Then, write to it
zipper.write(&mut file); // Amount of threads is chosen automatically
```
