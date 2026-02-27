#!/usr/bin/elle
## I/O Operations - Comprehensive Demonstration
## This example showcases all I/O primitives available in Elle
## 
## Note: Uses relative paths (./elle_example_*) to work in any environment,
## including CI systems.

(import-file "./examples/assertions.lisp")

(begin
  ## Cleanup from previous runs
  (var cleanup-files (list "./elle_example_basic.txt" "./elle_example_copy.txt" "./elle_example_renamed.txt" "./elle_example_lines.txt" "./elle_example_config.txt"))
  (var cleanup-dirs (list "./elle_example_dir" "./elle_example_data" "./elle_example_archive"))
  
  (display "=== I/O Operations in Elle ===")
  (newline)
  (newline)

  ## Part 1: Basic File Writing and Reading
  (display "Part 1: Basic File Writing and Reading")
  (newline)
  (display "---")
  (newline)

  (var temp-file "./elle_example_basic.txt")
  (spit temp-file "Hello, Elle!")
  (display "Wrote to file: ")
  (display temp-file)
  (newline)

  (var content (slurp temp-file))
  (display "Read from file: ")
  (display content)
  (newline)
  (assert-equal content "Hello, Elle!" "I/O: spit/slurp basic read-write")
  (newline)

  ## Part 2: File Existence Checking
  (display "Part 2: File Existence Checking")
  (newline)
  (display "---")
  (newline)

  (display "Does file exist? ")
  (let ((exists (file-exists? temp-file)))
    (display exists)
    (newline)
    (assert-true exists "I/O: file-exists? returns true for created file"))

  (display "Does nonexistent file exist? ")
  (let ((not-exists (file-exists? "./this_does_not_exist_12345.txt")))
    (display not-exists)
    (newline)
    (assert-true (not not-exists) "I/O: file-exists? returns false for nonexistent file"))
  (newline)

  ## Part 3: File Properties and Information
  (display "Part 3: File Properties and Information")
  (newline)
  (display "---")
  (newline)

  (display "File size: ")
  (let ((size (file-size temp-file)))
    (display size)
    (display " bytes")
    (newline)
    (assert-equal size 12 "I/O: file-size returns 12 for 'Hello, Elle!'"))

  (display "Is file? ")
  (let ((is-file (file? temp-file)))
    (display is-file)
    (newline)
    (assert-true is-file "I/O: file? returns true for file"))

  (display "Is directory? ")
  (let ((is-dir (directory? ".")))
    (display is-dir)
    (newline)
    (assert-true is-dir "I/O: directory? returns true for directory"))
  (newline)

  ## Part 4: Appending to Files
  (display "Part 4: Appending Content to Files")
  (newline)
  (display "---")
  (newline)

  (append-file temp-file "\nThis is appended content.")
  (display "Appended more content. New content:")
  (newline)
   (let ((appended-content (slurp temp-file)))
     (display appended-content)
     (newline)
     (assert-true (> (length appended-content) 12) "I/O: appended content is longer"))
  (newline)

  ## Part 5: File Copying
  (display "Part 5: File Copying")
  (newline)
  (display "---")
  (newline)

  (var copied-file "./elle_example_copy.txt")
  (copy-file temp-file copied-file)
  (display "Copied file to: ")
  (display copied-file)
  (newline)
  (display "Copy exists? ")
  (let ((copy-exists (file-exists? copied-file)))
    (display copy-exists)
    (newline)
    (assert-true copy-exists "I/O: copy-file creates new file"))
  (newline)

  ## Part 6: File Renaming
  (display "Part 6: File Renaming")
  (newline)
  (display "---")
  (newline)

  (var renamed-file "./elle_example_renamed.txt")
  (rename-file copied-file renamed-file)
  (display "Renamed to: ")
  (display renamed-file)
  (newline)
  (display "Old name exists? ")
  (let ((old-exists (file-exists? copied-file)))
    (display old-exists)
    (newline)
    (assert-true (not old-exists) "I/O: rename-file removes old name"))
  (display "New name exists? ")
  (let ((new-exists (file-exists? renamed-file)))
    (display new-exists)
    (newline)
    (assert-true new-exists "I/O: rename-file creates new name"))
  (newline)

  ## Part 7: Path Operations
  (display "Part 7: Path Operations")
  (newline)
  (display "---")
  (newline)

  (var full-path "/home/user/documents/project/report.pdf")

  (display "Full path: ")
  (display full-path)
  (newline)

  (display "File name: ")
  (display (file-name full-path))
  (newline)

  (display "File extension: ")
  (display (file-extension full-path))
  (newline)

  (display "Parent directory: ")
  (display (parent-directory full-path))
  (newline)

  (var composed-path (join-path "." "test" "nested" "file.txt"))
  (display "Composed path: ")
  (display composed-path)
  (newline)
  (newline)

  ## Part 8: Directory Operations
  (display "Part 8: Directory Operations")
  (newline)
  (display "---")
  (newline)

  (var test-dir "./elle_example_dir")
  (var sub-dir "./elle_example_dir/subdir")

  (create-directory test-dir)
  (display "Created directory: ")
  (display test-dir)
  (newline)

  (create-directory-all sub-dir)
  (display "Created subdirectory: ")
  (display sub-dir)
  (newline)

  (display "Directory exists? ")
  (let ((dir-exists (directory? test-dir)))
    (display dir-exists)
    (newline)
    (assert-true dir-exists "I/O: create-directory creates directory"))
  (newline)

  ## Part 9: Reading Files Line by Line
  (display "Part 9: Reading Files Line by Line")
  (newline)
  (display "---")
  (newline)

  (var lines-file "./elle_example_lines.txt")
  (spit lines-file "Line 1: First line of text\nLine 2: Second line of text\nLine 3: Third line of text\n")

  (display "Lines in file:")
  (newline)
  (var lines (read-lines lines-file))
  (display lines)
  (newline)
  (assert-equal (length lines) 3 "I/O: read-lines returns 3 lines")
  (newline)

  ## Part 10: Directory Listing
  (display "Part 10: Directory Listing")
  (newline)
  (display "---")
  (newline)

  ## Create some test files in a directory
  (spit (join-path test-dir "file1.txt") "Content 1")
  (spit (join-path test-dir "file2.txt") "Content 2")
  (spit (join-path test-dir "file3.txt") "Content 3")

  (display "Files in directory:")
  (newline)
  (let ((dir-list (list-directory test-dir)))
    (display dir-list)
    (newline)
    (assert-true (> (length dir-list) 0) "I/O: list-directory returns files"))
  (newline)

  ## Part 11: Working Directory Operations
  (display "Part 11: Working Directory Operations")
  (newline)
  (display "---")
  (newline)

   (display "Current directory: ")
   (let ((cwd (current-directory)))
     (display cwd)
     (newline)
     (assert-true (> (length cwd) 0) "I/O: current-directory returns valid path"))
  (newline)

  ## Part 12: Practical Example - Config File Handling
  (display "Part 12: Practical Example - Config File Handling")
  (newline)
  (display "---")
  (newline)

  (var config-file "./elle_example_config.txt")

  ## Write initial config
  (spit config-file "# Elle Configuration File\nversion=1.0\nauthor=Elle Users\n")
  (display "Created config file")
  (newline)

  ## Read and display config
  (display "Current config:")
  (newline)
   (var config-content (slurp config-file))
   (display config-content)
   (newline)
   (assert-true (> (length config-content) 0) "I/O: config file has content")

  ## Append new settings
  (append-file config-file "debug=true\nverbose=false\n")
  (display "Updated config file")
  (newline)

  ## Read updated version
  (display "Updated config:")
  (newline)
   (var updated-config (slurp config-file))
   (display updated-config)
   (newline)
   (assert-true (> (length updated-config) (length config-content)) "I/O: updated config is longer")
  (newline)

  ## Part 13: File Organization Example
  (display "Part 13: File Organization Example")
  (newline)
  (display "---")
  (newline)

  (var data-dir "./elle_example_data")
  (var archive-dir "./elle_example_archive")

  (create-directory-all data-dir)
  (create-directory-all archive-dir)

  ## Create some data files
  (spit (join-path data-dir "data1.txt") "Important data 1")
  (spit (join-path data-dir "data2.txt") "Important data 2")
  (spit (join-path data-dir "data3.txt") "Important data 3")

  (display "Organized data files in: ")
  (display data-dir)
  (newline)

  ## Archive a file (copy to archive directory)
  (var original (join-path data-dir "data1.txt"))
  (var archived (join-path archive-dir "data1_archived.txt"))
  (copy-file original archived)

  (display "Archived file to: ")
  (display archived)
  (newline)
  (display "Archived file exists? ")
  (let ((archived-exists (file-exists? archived)))
    (display archived-exists)
    (newline)
    (assert-true archived-exists "I/O: archived file exists after copy"))
  (newline)

  ## Part 14: Error Handling and Edge Cases
  (display "Part 14: Error Handling Considerations")
  (newline)
  (display "---")
  (newline)

  (display "The following operations gracefully handle errors:")
  (newline)
  (display "- Reading non-existent files returns an error")
  (newline)
  (display "- Writing to invalid paths returns an error")
  (newline)
  (display "- Deleting non-existent files returns an error")
  (newline)
  (newline)

  ## Part 15: Cleanup
  (display "Part 15: Cleanup Operations")
  (newline)
  (display "---")
  (newline)

  (delete-file temp-file)
  (display "Deleted: ")
  (display temp-file)
  (newline)

  (delete-file renamed-file)
  (display "Deleted: ")
  (display renamed-file)
  (newline)

  (delete-file lines-file)
  (display "Deleted: ")
  (display lines-file)
  (newline)

  (delete-file config-file)
  (display "Deleted: ")
  (display config-file)
  (newline)

  ## Delete files in test directories
  (delete-file (join-path data-dir "data1.txt"))
  (delete-file (join-path data-dir "data2.txt"))
  (delete-file (join-path data-dir "data3.txt"))
  (delete-file (join-path archive-dir "data1_archived.txt"))
  (delete-file (join-path test-dir "file1.txt"))
  (delete-file (join-path test-dir "file2.txt"))
  (delete-file (join-path test-dir "file3.txt"))
  (delete-directory (join-path test-dir "subdir"))
  (delete-directory data-dir)
  (delete-directory archive-dir)
  (delete-directory test-dir)

  (display "Cleanup complete")
  (newline)
  (newline)

  ## Summary
  (display "=== Summary of I/O Functions ===")
  (newline)
  (newline)

  (display "Reading and Writing:")
  (newline)
  (display "  (slurp path) - Read entire file as string")
  (newline)
  (display "  (spit path content) - Write or overwrite file")
  (newline)
  (display "  (append-file path content) - Append to file")
  (newline)
  (display "  (read-lines path) - Read file as list of lines")
  (newline)
  (newline)

  (display "File Information:")
  (newline)
  (display "  (file-exists? path) - Check if file exists")
  (newline)
  (display "  (file? path) - Check if path is a file")
  (newline)
  (display "  (directory? path) - Check if path is a directory")
  (newline)
  (display "  (file-size path) - Get file size in bytes")
  (newline)
  (newline)

  (display "File Operations:")
  (newline)
  (display "  (delete-file path) - Delete a file")
  (newline)
  (display "  (rename-file old-path new-path) - Rename file")
  (newline)
  (display "  (copy-file src dest) - Copy file")
  (newline)
  (newline)

  (display "Directory Operations:")
  (newline)
  (display "  (create-directory path) - Create single directory")
  (newline)
  (display "  (create-directory-all path) - Create with parents")
  (newline)
  (display "  (delete-directory path) - Delete empty directory")
  (newline)
  (display "  (list-directory path) - List directory contents")
  (newline)
  (newline)

  (display "Path Operations:")
  (newline)
  (display "  (file-name path) - Extract filename")
  (newline)
  (display "  (file-extension path) - Extract file extension")
  (newline)
  (display "  (parent-directory path) - Get parent directory")
  (newline)
  (display "  (join-path ...parts) - Join path components")
  (newline)
  (display "  (absolute-path path) - Get absolute path")
  (newline)
  (display "  (current-directory) - Get working directory")
  (newline)
  (display "  (change-directory path) - Change working directory")
  (newline)
  (newline)

  (display "=== I/O Example Complete ===")
  (newline)
  (display "=== I/O Assertions Complete ===")
  (newline))
