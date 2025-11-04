use std::sync::RwLockWriteGuard;

use crate::sys::{OIDNDevice, oidnRetainDevice, oidnSyncDevice};

/// A type stating that a sync must happen typically this type will
/// mutably borrow a resource. This type will synchronise with the
/// device either
/// 
/// a) when this is dropped or,
/// 
/// b) when `Self::synchronise_now` is called.
/// 
/// ### Warning
/// Calling [`std::mem::forget`] (or getting around dropping) on
/// this will leave the resource that is used by this in an
/// unusable state, crashing the process apon usage.
pub struct MustSync<'a, R> {
    raw_device: OIDNDevice,
    _lock: RwLockWriteGuard<'a, ()>,
    /// function to be called after sync has happened.
    /// 
    /// Must be `Some`, Option so that function can be taken and executed.
    func: Option<Box<dyn FnOnce() -> R>>,
}

/// A lock to prevent async usages
// A read/write lock so we can check that it isn't being written
// to (i.e. resource needs sync) without taking the write guard and
// potentially crashing other threads
#[derive(Default)]
pub struct SyncLock(std::sync::RwLock<()>);

impl SyncLock {
    /// Checks that the mutex is unlocked
    pub fn check_valid(&self) {
        self.0.clear_poison();
        // We don't actually care about the lock, just that this is unlocked.
        #[allow(let_underscore_lock)]
        let _ = self.0.try_read().expect("The user has executed `mem::forget` on a `MustSync` for this resource. A `MustSync` must have its destructor called.");
    }
}

impl<R> Drop for MustSync<'_, R> {
    fn drop(&mut self) {
        unsafe { oidnSyncDevice(self.raw_device) };
    }
}

impl<R> MustSync<'_, R> {
    /// Synchronises with the oidn device now, blocking until it is finished
    pub fn synchronise_now(mut self) -> R {
        unsafe { oidnSyncDevice(self.raw_device) };
        (self.func.take().expect("All creation points should set this to be Some, and we drop this immediately"))()
    }

    /// Return a [`MustSync`] object for this the inputted device 
    ///
    /// This is for library developers who need to require a sync,
    /// **not** for users. All methods that need to be syncronised should
    /// return [`MustSync`].
    /// 
    /// `func` is executed after the synchronisation, and it should have no
    /// side effects, it may not be executed if the value isn't required.
    pub fn must_sync<'a>(device: OIDNDevice, sync_lock: &'a mut SyncLock, func: Box<dyn FnOnce() -> R>) -> crate::MustSync<'a, R> {
        unsafe { oidnRetainDevice(device) };
        sync_lock.check_valid();
        sync_lock.0.clear_poison();
        let lock = sync_lock.0.write().unwrap();
        MustSync { raw_device: device, _lock: lock, func: Some(func) }
    }
}