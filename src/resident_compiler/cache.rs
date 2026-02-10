//! Disk and memory caching for compiled documents

use super::compiled_doc::CompiledDocument;
use std::collections::HashMap;
use std::path::PathBuf;

/// Disk-based cache in /dev/shm for persistence across CLI invocations
pub struct DiskCache {
    cache_dir: PathBuf,
}

impl DiskCache {
    /// Create a new disk cache
    pub fn new() -> Self {
        let cache_dir = PathBuf::from("/dev/shm/elle");
        let _ = std::fs::create_dir_all(&cache_dir);
        Self { cache_dir }
    }

    /// Get a cached document if it exists and is valid
    pub fn get(&self, path: &str) -> Option<CompiledDocument> {
        let key = Self::hash_path(path);
        let entry_dir = self.cache_dir.join(&key);

        // Read metadata to check if cached version is still valid
        let metadata_path = entry_dir.join("metadata.json");
        if !metadata_path.exists() {
            return None;
        }

        // For now, return None - full serialization will be added later
        // This is just the structure to enable caching
        None
    }

    /// Put a document in the cache
    pub fn put(&self, path: &str, _doc: &CompiledDocument) {
        let key = Self::hash_path(path);
        let _entry_dir = self.cache_dir.join(&key);

        // TODO: Implement actual serialization
        // For now, cache infrastructure is in place for future use
    }

    /// Remove a cached document
    pub fn remove(&self, path: &str) {
        let key = Self::hash_path(path);
        let _ = std::fs::remove_dir_all(self.cache_dir.join(&key));
    }

    /// Clear entire disk cache
    pub fn clear(&self) {
        let _ = std::fs::remove_dir_all(&self.cache_dir);
    }

    /// Hash a file path to create a cache key
    fn hash_path(path: &str) -> String {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        path.hash(&mut hasher);
        format!("{:x}", hasher.finish())
    }
}

impl Default for DiskCache {
    fn default() -> Self {
        Self::new()
    }
}

/// In-memory cache for unsaved buffers and REPL expressions
pub struct MemoryCache {
    entries: HashMap<String, CompiledDocument>,
}

impl MemoryCache {
    /// Create a new memory cache
    pub fn new() -> Self {
        Self {
            entries: HashMap::new(),
        }
    }

    /// Get a cached document
    pub fn get(&self, name: &str) -> Option<CompiledDocument> {
        self.entries.get(name).cloned()
    }

    /// Put a document in the cache
    pub fn put(&mut self, name: String, doc: CompiledDocument) {
        self.entries.insert(name, doc);
    }

    /// Remove a cached document
    pub fn remove(&mut self, name: &str) {
        self.entries.remove(name);
    }

    /// Clear entire memory cache
    pub fn clear(&mut self) {
        self.entries.clear();
    }

    /// Get the number of cached entries
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Check if cache is empty
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

impl Default for MemoryCache {
    fn default() -> Self {
        Self::new()
    }
}
