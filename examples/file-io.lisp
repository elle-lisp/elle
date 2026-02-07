#!/usr/bin/elle
;; File I/O Example - Comprehensive Demonstration
;; This example showcases all file I/O primitives available in Elle
;; 
;; Note: Uses relative paths (./elle_example_*) to work in any environment,
;; including CI systems.

(begin
  (display "=== File I/O Operations in Elle ===")
  (newline)
  (newline)

  ;; Part 1: Basic File Writing and Reading
  (display "Part 1: Basic File Writing and Reading")
  (newline)
  (display "---")
  (newline)

  (define temp-file "./elle_example_basic.txt")
  (write-file temp-file "Hello, Elle!")
  (display "Wrote to file: ")
  (display temp-file)
  (newline)

  (define content (read-file temp-file))
  (display "Read from file: ")
  (display content)
  (newline)
  (newline)

  ;; Part 2: File Existence Checking
  (display "Part 2: File Existence Checking")
  (newline)
  (display "---")
  (newline)

  (display "Does file exist? ")
  (display (file-exists? temp-file))
  (newline)

  (display "Does nonexistent file exist? ")
  (display (file-exists? "./this_does_not_exist_12345.txt"))
  (newline)
  (newline)

  ;; Part 3: File Properties and Information
  (display "Part 3: File Properties and Information")
  (newline)
  (display "---")
  (newline)

  (display "File size: ")
  (display (file-size temp-file))
  (display " bytes")
  (newline)

  (display "Is file? ")
  (display (file? temp-file))
  (newline)

  (display "Is directory? ")
  (display (directory? "."))
  (newline)
  (newline)

  ;; Part 4: Appending to Files
  (display "Part 4: Appending Content to Files")
  (newline)
  (display "---")
  (newline)

  (append-file temp-file "\nThis is appended content.")
  (display "Appended more content. New content:")
  (newline)
  (display (read-file temp-file))
  (newline)
  (newline)

  ;; Part 5: File Copying
  (display "Part 5: File Copying")
  (newline)
  (display "---")
  (newline)

  (define copied-file "./elle_example_copy.txt")
  (copy-file temp-file copied-file)
  (display "Copied file to: ")
  (display copied-file)
  (newline)
  (display "Copy exists? ")
  (display (file-exists? copied-file))
  (newline)
  (newline)

  ;; Part 6: File Renaming
  (display "Part 6: File Renaming")
  (newline)
  (display "---")
  (newline)

  (define renamed-file "./elle_example_renamed.txt")
  (rename-file copied-file renamed-file)
  (display "Renamed to: ")
  (display renamed-file)
  (newline)
  (display "Old name exists? ")
  (display (file-exists? copied-file))
  (newline)
  (display "New name exists? ")
  (display (file-exists? renamed-file))
  (newline)
  (newline)

  ;; Part 7: Path Operations
  (display "Part 7: Path Operations")
  (newline)
  (display "---")
  (newline)

  (define full-path "/home/user/documents/project/report.pdf")

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

  (define composed-path (join-path "." "test" "nested" "file.txt"))
  (display "Composed path: ")
  (display composed-path)
  (newline)
  (newline)

  ;; Part 8: Directory Operations
  (display "Part 8: Directory Operations")
  (newline)
  (display "---")
  (newline)

  (define test-dir "./elle_example_dir")
  (define sub-dir "./elle_example_dir/subdir")

  (create-directory test-dir)
  (display "Created directory: ")
  (display test-dir)
  (newline)

  (create-directory-all sub-dir)
  (display "Created subdirectory: ")
  (display sub-dir)
  (newline)

  (display "Directory exists? ")
  (display (directory? test-dir))
  (newline)
  (newline)

  ;; Part 9: Reading Files Line by Line
  (display "Part 9: Reading Files Line by Line")
  (newline)
  (display "---")
  (newline)

  (define lines-file "./elle_example_lines.txt")
  (write-file lines-file "Line 1: First line of text\nLine 2: Second line of text\nLine 3: Third line of text\n")

  (display "Lines in file:")
  (newline)
  (define lines (read-lines lines-file))
  (display lines)
  (newline)
  (newline)

  ;; Part 10: Directory Listing
  (display "Part 10: Directory Listing")
  (newline)
  (display "---")
  (newline)

  ;; Create some test files in a directory
  (write-file (join-path test-dir "file1.txt") "Content 1")
  (write-file (join-path test-dir "file2.txt") "Content 2")
  (write-file (join-path test-dir "file3.txt") "Content 3")

  (display "Files in directory:")
  (newline)
  (display (list-directory test-dir))
  (newline)
  (newline)

  ;; Part 11: Working Directory Operations
  (display "Part 11: Working Directory Operations")
  (newline)
  (display "---")
  (newline)

  (display "Current directory: ")
  (display (current-directory))
  (newline)
  (newline)

  ;; Part 12: Practical Example - Config File Handling
  (display "Part 12: Practical Example - Config File Handling")
  (newline)
  (display "---")
  (newline)

  (define config-file "./elle_example_config.txt")

  ;; Write initial config
  (write-file config-file "# Elle Configuration File\nversion=1.0\nauthor=Elle Users\n")
  (display "Created config file")
  (newline)

  ;; Read and display config
  (display "Current config:")
  (newline)
  (display (read-file config-file))
  (newline)

  ;; Append new settings
  (append-file config-file "debug=true\nverbose=false\n")
  (display "Updated config file")
  (newline)

  ;; Read updated version
  (display "Updated config:")
  (newline)
  (display (read-file config-file))
  (newline)
  (newline)

  ;; Part 13: File Organization Example
  (display "Part 13: File Organization Example")
  (newline)
  (display "---")
  (newline)

  (define data-dir "./elle_example_data")
  (define archive-dir "./elle_example_archive")

  (create-directory-all data-dir)
  (create-directory-all archive-dir)

  ;; Create some data files
  (write-file (join-path data-dir "data1.txt") "Important data 1")
  (write-file (join-path data-dir "data2.txt") "Important data 2")
  (write-file (join-path data-dir "data3.txt") "Important data 3")

  (display "Organized data files in: ")
  (display data-dir)
  (newline)

  ;; Archive a file (copy to archive directory)
  (define original (join-path data-dir "data1.txt"))
  (define archived (join-path archive-dir "data1_archived.txt"))
  (copy-file original archived)

  (display "Archived file to: ")
  (display archived)
  (newline)
  (display "Archived file exists? ")
  (display (file-exists? archived))
  (newline)
  (newline)

  ;; Part 14: Error Handling and Edge Cases
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

  ;; Part 15: Cleanup
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

  ;; Delete files in test directories
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

  ;; Summary
  (display "=== Summary of File I/O Functions ===")
  (newline)
  (newline)

  (display "Reading and Writing:")
  (newline)
  (display "  (read-file path) - Read entire file as string")
  (newline)
  (display "  (write-file path content) - Write or overwrite file")
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

  (display "=== File I/O Example Complete ===")
  (newline))
