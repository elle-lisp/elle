//! ResidentCompiler - shared compilation interface for LSP and CLI

use super::cache::{DiskCache, MemoryCache};
use super::compiled_doc::CompiledDocument;
use crate::compiler::ast::ExprWithLoc;
use crate::compiler::{compile_with_metadata, extract_symbols, Linter};
use crate::reader::{Lexer, Reader};
use crate::{init_stdlib, register_primitives, SymbolTable, VM};

/// Error type for resident compiler operations
#[derive(Debug, Clone)]
pub struct CompileError {
    pub message: String,
}

impl std::fmt::Display for CompileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for CompileError {}

/// Resident compiler that caches compiled expressions
///
/// Maintains a shared compilation interface for both LSP and CLI,
/// caching compiled expressions to reduce work in edit/eval loops.
pub struct ResidentCompiler {
    disk_cache: DiskCache,
    memory_cache: MemoryCache,
    symbol_table: SymbolTable,
    #[allow(dead_code)]
    vm: VM,
}

impl ResidentCompiler {
    /// Create a new resident compiler with initialized stdlib
    pub fn new() -> Self {
        let mut symbol_table = SymbolTable::new();
        let mut vm = VM::new();
        register_primitives(&mut vm, &mut symbol_table);
        init_stdlib(&mut vm, &mut symbol_table);

        Self {
            disk_cache: DiskCache::new(),
            memory_cache: MemoryCache::new(),
            symbol_table,
            vm,
        }
    }

    /// Compile a file from disk with caching
    pub fn compile_file(&mut self, path: &str) -> Result<CompiledDocument, CompileError> {
        // Check if we have a valid cached version
        if let Some(cached) = self.disk_cache.get(path) {
            if cached.is_valid_for_file(path) {
                return Ok(cached);
            }
        }

        // Read file from disk
        let source = std::fs::read_to_string(path).map_err(|e| CompileError {
            message: format!("Failed to read {}: {}", path, e),
        })?;

        // Compile the text
        let doc = self.compile_text(path, &source)?;

        // Cache and return
        self.disk_cache.put(path, &doc);
        Ok(doc)
    }

    /// Compile text (for unsaved buffers, REPL, etc.)
    pub fn compile_text(
        &mut self,
        name: &str,
        text: &str,
    ) -> Result<CompiledDocument, CompileError> {
        // Check memory cache first
        if let Some(cached) = self.memory_cache.get(name) {
            if cached.source_text == text {
                return Ok(cached);
            }
        }

        // Create lexer with file information
        let lexer = Lexer::with_file(text, name);

        // Collect tokens
        let mut tokens = Vec::new();
        let mut lex = lexer;
        loop {
            match lex.next_token() {
                Ok(Some(token)) => {
                    tokens.push(crate::reader::OwnedToken::from(token));
                }
                Ok(None) => break,
                Err(e) => {
                    return Err(CompileError {
                        message: format!("Lexer error: {}", e),
                    });
                }
            }
        }

        // Parse into Value
        let mut reader = Reader::new(tokens);
        let value = reader
            .read(&mut self.symbol_table)
            .map_err(|e| CompileError {
                message: format!("Parse error: {}", e),
            })?;

        // Convert Value to Expr (self.symbol_table is already mutable from &mut self)
        let expr = crate::compiler::converters::value_to_expr(&value, &mut self.symbol_table)
            .map_err(|e| CompileError {
                message: format!("Conversion error: {}", e),
            })?;

        // Create wrapped expression (for now, without location since we lose it in conversion)
        let expr_with_loc = ExprWithLoc {
            expr: expr.clone(),
            loc: None,
        };

        // Compile with metadata
        let (bytecode, location_map) = compile_with_metadata(&expr, None);

        // Extract symbols for IDE
        let symbols = extract_symbols(std::slice::from_ref(&expr_with_loc), &self.symbol_table);

        // Run linter
        let mut linter = Linter::new();
        linter.lint_expr(&expr_with_loc, &self.symbol_table);
        let diagnostics = linter.diagnostics();

        let doc = CompiledDocument::new(
            text.to_string(),
            expr_with_loc,
            bytecode,
            location_map,
            symbols,
            diagnostics.to_vec(),
        );

        // Cache and return
        self.memory_cache.put(name.to_string(), doc.clone());
        Ok(doc)
    }

    /// Get a cached document without recompiling
    pub fn get_cached(&self, name: &str) -> Option<CompiledDocument> {
        self.memory_cache.get(name)
    }

    /// Invalidate a cached document
    pub fn invalidate(&mut self, name: &str) {
        self.memory_cache.remove(name);
        self.disk_cache.remove(name);
    }

    /// Clear all disk cache entries
    pub fn invalidate_all_disk(&self) {
        self.disk_cache.clear();
    }

    /// Get reference to symbol table for symbol lookup
    pub fn symbols(&self) -> &SymbolTable {
        &self.symbol_table
    }

    /// Get mutable reference to symbol table (for stdlib modifications)
    pub fn symbols_mut(&mut self) -> &mut SymbolTable {
        &mut self.symbol_table
    }
}

impl Default for ResidentCompiler {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resident_compiler_creation() {
        let _compiler = ResidentCompiler::new();
        // Constructor should succeed
    }

    #[test]
    fn test_compile_simple_expression() {
        let mut compiler = ResidentCompiler::new();
        let result = compiler.compile_text("test", "(+ 1 2)");
        assert!(result.is_ok());

        let doc = result.unwrap();
        assert_eq!(doc.source_text, "(+ 1 2)");
        assert!(!doc.bytecode.instructions.is_empty());
    }

    #[test]
    fn test_memory_cache_hit() {
        let mut compiler = ResidentCompiler::new();

        // First compile
        let result1 = compiler.compile_text("test", "(+ 1 2)");
        assert!(result1.is_ok());

        // Second compile (should be cached)
        let result2 = compiler.compile_text("test", "(+ 1 2)");
        assert!(result2.is_ok());

        // Cache should have the entry
        let cached = compiler.get_cached("test");
        assert!(cached.is_some());
    }

    #[test]
    fn test_cache_invalidation() {
        let mut compiler = ResidentCompiler::new();

        let _result = compiler.compile_text("test", "(+ 1 2)");
        assert!(compiler.get_cached("test").is_some());

        compiler.invalidate("test");
        assert!(compiler.get_cached("test").is_none());
    }
}
