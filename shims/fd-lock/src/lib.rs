//! Minimal shim of the fd-lock crate for wasm builds and other non-Unix targets.
//! This crate intentionally implements a permissive, no-op locking API that compiles
//! where the real `fd-lock` is platform-specific (e.g., unix-only code using AsFd).
//! It aims for minimal compatibility to satisfy crates that depend on fd-lock, while
//! implementing safe no-op semantics on wasm targets.

use std::ops::Deref;
use parking_lot::RwLock as ParkingRwLock;
use std::sync::Arc;

// Provide a minimal AsOpenFile trait to match expected trait bound names.
pub trait AsOpenFile {}

// Blanket implementations for common types are unnecessary for wasm builds, but
// this preserves the trait for generic constraints.

// RwLock: a simple wrapper around parking_lot::RwLock providing a similar API
// to fd-lock's RwLock with Read/Write guards. These are no-op wrappers for the
// purposes of wasm builds but satisfy the type system.

pub struct RwLock<T> {
    inner: Arc<ParkingRwLock<T>>,
}

impl<T> RwLock<T> {
    pub fn new(inner: T) -> Self {
        Self { inner: Arc::new(ParkingRwLock::new(inner)) }
    }

    pub fn read(&self) -> std::io::Result<RwLockReadGuard<'_, T>> {
        Ok(RwLockReadGuard { guard: self.inner.read() })
    }

    pub fn write(&self) -> std::io::Result<RwLockWriteGuard<'_, T>> {
        Ok(RwLockWriteGuard { guard: self.inner.write() })
    }
}

pub struct RwLockReadGuard<'a, T> {
    guard: parking_lot::RwLockReadGuard<'a, T>,
}

impl<'a, T> Deref for RwLockReadGuard<'a, T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        &*self.guard
    }
}

pub struct RwLockWriteGuard<'a, T> {
    guard: parking_lot::RwLockWriteGuard<'a, T>,
}

impl<'a, T> Deref for RwLockWriteGuard<'a, T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        &*self.guard
    }
}

impl<'a, T> std::ops::DerefMut for RwLockWriteGuard<'a, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut *self.guard
    }
}

// Re-export the names to keep compatibility with fd-lock API surface
pub use RwLock as FdRwLock;
pub use RwLockReadGuard as FdRwLockReadGuard;
pub use RwLockWriteGuard as FdRwLockWriteGuard;

// Provide no-op read / write lock functions for compatibility

