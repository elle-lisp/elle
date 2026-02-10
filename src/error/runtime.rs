//! Runtime error with location and context information

use super::sourceloc::SourceLoc;
use std::fmt;

/// Runtime error with optional source location
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeError {
    pub message: String,
    pub location: Option<SourceLoc>,
    pub context: Option<String>,
}

impl RuntimeError {
    /// Create a new runtime error
    pub fn new(message: String) -> Self {
        RuntimeError {
            message,
            location: None,
            context: None,
        }
    }

    /// Add location information
    pub fn with_location(mut self, location: SourceLoc) -> Self {
        self.location = Some(location);
        self
    }

    /// Add context information
    pub fn with_context(mut self, context: String) -> Self {
        self.context = Some(context);
        self
    }
}

impl fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.location {
            Some(loc) => write!(f, "Error at {}: {}", loc, self.message)?,
            None => write!(f, "Error: {}", self.message)?,
        }

        if let Some(ref ctx) = self.context {
            write!(f, "\n  Context: {}", ctx)?;
        }

        Ok(())
    }
}

impl std::error::Error for RuntimeError {}
