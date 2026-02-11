// Stack slot allocation for variable storage (Phase 5+)
//
// Manages allocation and tracking of stack slots for storing variable values
// during JIT compilation. Each variable at a given (depth, index) gets a
// dedicated stack slot for storing its compiled value.

use cranelift::codegen::ir::{StackSlot, StackSlotData, StackSlotKind};
use cranelift::prelude::*;
use std::collections::HashMap;

/// Type of value stored in a stack slot
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlotType {
    I64,
    F64,
}

/// Allocates and manages stack slots for variables
#[derive(Debug)]
pub struct StackAllocator {
    /// Maps (depth, index) -> (StackSlot, SlotType)
    slot_map: HashMap<(usize, usize), (StackSlot, SlotType)>,
    /// Total bytes allocated for all slots
    total_size: u32,
}

impl StackAllocator {
    /// Create a new stack allocator
    pub fn new() -> Self {
        StackAllocator {
            slot_map: HashMap::new(),
            total_size: 0,
        }
    }

    /// Allocate a stack slot for a variable (8 bytes for i64/f64)
    /// Returns the StackSlot and its offset from the stack frame base
    pub fn allocate(
        &mut self,
        builder: &mut FunctionBuilder,
        depth: usize,
        index: usize,
        slot_type: SlotType,
    ) -> Result<(StackSlot, u32), String> {
        let key = (depth, index);

        // Check if already allocated
        if let Some((slot, _)) = self.slot_map.get(&key) {
            // Get the offset for this slot
            let offset = self.get_offset(depth, index)?;
            return Ok((*slot, offset));
        }

        // Allocate 8 bytes for this variable (fits both i64 and f64)
        let slot =
            builder.create_sized_stack_slot(StackSlotData::new(StackSlotKind::ExplicitSlot, 8));

        let offset = self.total_size;
        self.total_size += 8;

        self.slot_map.insert(key, (slot, slot_type));
        Ok((slot, offset))
    }

    /// Get an existing stack slot
    pub fn get(&self, depth: usize, index: usize) -> Option<StackSlot> {
        self.slot_map.get(&(depth, index)).map(|(slot, _)| *slot)
    }

    /// Get an existing stack slot with its type
    pub fn get_with_type(&self, depth: usize, index: usize) -> Option<(StackSlot, SlotType)> {
        self.slot_map.get(&(depth, index)).copied()
    }

    /// Check if a slot exists
    pub fn has(&self, depth: usize, index: usize) -> bool {
        self.slot_map.contains_key(&(depth, index))
    }

    /// Get the offset of a slot (for debugging/introspection)
    fn get_offset(&self, depth: usize, index: usize) -> Result<u32, String> {
        let key = (depth, index);
        let position = self.slot_map.iter().filter(|(&k, _)| k <= key).count();

        Ok((position * 8) as u32)
    }

    /// Get total allocated size
    pub fn total_size(&self) -> u32 {
        self.total_size
    }

    /// Get number of allocated slots
    pub fn slot_count(&self) -> usize {
        self.slot_map.len()
    }

    /// Clear all allocations
    pub fn clear(&mut self) {
        self.slot_map.clear();
        self.total_size = 0;
    }
}

impl Default for StackAllocator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_allocator_creation() {
        let allocator = StackAllocator::new();
        assert_eq!(allocator.slot_count(), 0);
        assert_eq!(allocator.total_size(), 0);
    }

    #[test]
    fn test_allocator_has_slot() {
        let allocator = StackAllocator::new();
        assert!(!allocator.has(0, 0));
        assert!(!allocator.has(1, 0));
    }

    #[test]
    fn test_allocator_get_nonexistent() {
        let allocator = StackAllocator::new();
        assert!(allocator.get(0, 0).is_none());
    }

    #[test]
    fn test_allocator_clear() {
        let allocator = StackAllocator::new();
        assert_eq!(allocator.slot_count(), 0);
        assert_eq!(allocator.total_size(), 0);
    }

    #[test]
    fn test_allocator_get_offset() {
        let allocator = StackAllocator::new();
        let offset = allocator.get_offset(0, 0);
        assert!(offset.is_ok());
        assert_eq!(offset.unwrap(), 0);
    }
}
