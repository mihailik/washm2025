use crate::interpreter::stack::Stack;
use tinywasm_types::{ModuleInstanceAddr, WasmValue, ContAddr};
use alloc::vec::Vec;

/// A stack instance in the store (for stack-switching)
///
/// Represents an allocated stack/continuation that can be switched to
#[derive(Debug)]
pub(crate) struct StackInstance {
    /// The stack itself (call stack, value stack, block stack)
    pub(crate) stack: Stack,
    
    /// The module instance this stack belongs to
    pub(crate) module_addr: ModuleInstanceAddr,
    
    /// The state of this stack
    pub(crate) state: StackState,
    
    /// Suspend context - stores information when suspended
    pub(crate) suspend_context: Option<SuspendContext>,
    /// Bound arguments created via `cont.bind`. These should be provided to the continuation when resumed.
    /// Each entry is (result_local_index, value) where the index is the local index in the target frame
    /// where the bound argument should be applied.
    pub(crate) bound_args: Option<Vec<(u32, WasmValue)>>,
}

/// The state of a stack instance
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StackState {
    /// The stack is currently running
    Running,
    
    /// The stack is suspended and can be resumed
    Suspended,
    
    /// The stack has completed execution
    Dead,
}

/// Context information for a suspension
#[derive(Debug, Clone)]
pub(crate) struct SuspendContext {
    /// The tag index used for suspension
    pub(crate) tag_index: u32,
    
    /// Values passed through the tag mechanism
    pub(crate) values: TagValue,
    
    /// Optional return continuation address
    pub(crate) return_addr: Option<ContAddr>,
}

/// Values passed through suspend/resume operations
#[derive(Debug, Clone)]
pub(crate) struct TagValue {
    /// The values being passed
    pub(crate) values: Vec<WasmValue>,
}

impl TagValue {
    /// Create an empty tag value
    pub(crate) fn empty() -> Self {
        Self { values: Vec::new() }
    }
    
    /// Create a tag value from a vector of values
    pub(crate) fn new(values: Vec<WasmValue>) -> Self {
        Self { values }
    }
}

impl StackInstance {
    /// Create a new stack instance
    pub(crate) fn new(stack: Stack, module_addr: ModuleInstanceAddr) -> Self {
        Self {
            stack,
            module_addr,
            state: StackState::Suspended, // New stacks start suspended
            suspend_context: None,
            bound_args: None,
        }
    }
    
    /// Create a new suspended stack instance with context
    #[cfg(feature = "stack-switching")]
    pub(crate) fn new_suspended(
        stack: Stack,
        module_addr: ModuleInstanceAddr,
        suspend_context: SuspendContext,
    ) -> Self {
        Self {
            stack,
            module_addr,
            state: StackState::Suspended,
            suspend_context: Some(suspend_context),
            bound_args: None,
        }
    }
    
    /// Mark as suspended with context
    pub(crate) fn suspend(&mut self, context: SuspendContext) {
        self.state = StackState::Suspended;
        self.suspend_context = Some(context);
    }
    
    /// Mark as running and clear suspend context
    pub(crate) fn resume(&mut self) -> Option<SuspendContext> {
        self.state = StackState::Running;
        self.suspend_context.take()
    }
    
    /// Mark as dead
    pub(crate) fn mark_dead(&mut self) {
        self.state = StackState::Dead;
        self.suspend_context = None;
    }
}
