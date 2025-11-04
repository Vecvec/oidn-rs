use crate::must_sync::SyncLock;
use crate::sys::{
    OIDNBuffer, OIDNDevice, oidnGetBufferSize, oidnNewBuffer, oidnReadBuffer, oidnReadBufferAsync,
    oidnReleaseBuffer, oidnRetainDevice, oidnSyncDevice, oidnWriteBuffer, oidnWriteBufferAsync,
};
use crate::{Device, MustSync};
use std::mem;
use std::sync::Arc;

pub struct Buffer {
    pub(crate) buf: OIDNBuffer,
    pub(crate) size: usize,
    pub(crate) device_arc: Arc<u8>,
    pub(crate) sync_lock: SyncLock,
    // A `retain`ed copy of the device we're using.
    pub(crate) device: OIDNDevice,
}

impl Device {
    /// Creates a new buffer from a slice, returns None if buffer creation
    /// failed
    pub fn create_buffer(&self, contents: &[f32]) -> Option<Buffer> {
        let byte_size = mem::size_of_val(contents);
        let buffer = unsafe {
            let buf = oidnNewBuffer(self.0, byte_size);
            if buf.is_null() {
                return None;
            } else {
                oidnWriteBuffer(buf, 0, byte_size, contents.as_ptr() as *const _);
                buf
            }
        };
        unsafe {
            oidnRetainDevice(self.0);
        }
        Some(Buffer {
            buf: buffer,
            size: contents.len(),
            device_arc: self.1.clone(),
            sync_lock: SyncLock::default(),
            device: self.0,
        })
    }
    /// # Safety
    /// Raw buffer must not be invalid (e.g. destroyed, null ect.)
    ///
    /// Raw buffer must have been created by this device
    ///
    /// This buffer must be unique (no other `Buffer` may be created from this)
    ///
    /// You not use both this Buffer and the raw buffer at the same time.
    pub unsafe fn create_buffer_from_raw(&self, buffer: OIDNBuffer) -> Buffer {
        let size = unsafe { oidnGetBufferSize(buffer) } / mem::size_of::<f32>();
        unsafe {
            oidnRetainDevice(self.0);
        }
        Buffer {
            buf: buffer,
            size,
            device_arc: self.1.clone(),
            sync_lock: SyncLock::default(),
            device: self.0,
        }
    }

    pub(crate) fn same_device_as_buf(&self, buf: &Buffer) -> bool {
        self.1.as_ref() as *const _ as isize == buf.device_arc.as_ref() as *const _ as isize
    }

    /// Writes asyncronously to the buffer, returns [None] if the sizes mismatch.
    ///
    /// Will prevent a user from writing to the buffer until they synchronise with this
    pub fn write_buffer_async<'s>(
        &self,
        buffer: &'s mut Buffer,
        contents: &'s [f32],
    ) -> Option<MustSync<'s, ()>> {
        buffer.sync_lock.check_valid();
        if buffer.size != contents.len() {
            None
        } else {
            let byte_size = mem::size_of_val(contents);
            unsafe {
                oidnWriteBufferAsync(buffer.buf, 0, byte_size, contents.as_ptr() as *const _);
            }
            Some(MustSync::must_sync(
                self.0,
                &mut buffer.sync_lock,
                Box::new(|| ()),
            ))
        }
    }

    /// Reads asyncronously from the buffer to an array.
    ///
    /// Will prevent a user from writing to the buffer until they syncronise with this
    // Note: this is mutable becuase oidn states that we cannot access the contents of this buffer until after this has synchronised
    pub fn read_buffer_async<'s>(&self, buffer: &'s mut Buffer) -> MustSync<'s, Vec<f32>> {
        buffer.sync_lock.check_valid();

        let mut vec = Vec::with_capacity(buffer.size);
        unsafe {
            oidnReadBufferAsync(
                buffer.buf,
                0,
                buffer.size * size_of::<f32>(),
                vec.as_mut_ptr() as *mut _,
            );
        }
        let size = buffer.size;
        MustSync::must_sync(
            self.0,
            &mut buffer.sync_lock,
            Box::new(move || unsafe {
                let mut vec = vec;
                // # Safety: we have read asynchronously to this
                vec.set_len(size);
                vec
            }),
        )
    }
}

impl Buffer {
    /// Writes to the buffer, returns [None] if the sizes mismatch
    pub fn write(&self, contents: &[f32]) -> Option<()> {
        self.sync_lock.check_valid();
        if self.size != contents.len() {
            None
        } else {
            let byte_size = mem::size_of_val(contents);
            unsafe {
                oidnWriteBuffer(self.buf, 0, byte_size, contents.as_ptr() as *const _);
            }
            Some(())
        }
    }

    /// Reads from the buffer to the array, returns [None] if the sizes mismatch
    pub fn read_to_slice(&self, contents: &mut [f32]) -> Option<()> {
        self.sync_lock.check_valid();
        if self.size != contents.len() {
            None
        } else {
            let byte_size = mem::size_of_val(contents);
            unsafe {
                oidnReadBuffer(self.buf, 0, byte_size, contents.as_mut_ptr() as *mut _);
            }
            Some(())
        }
    }

    /// Reads from the buffer
    pub fn read(&self) -> Vec<f32> {
        self.sync_lock.check_valid();
        let mut contents = vec![0.0; self.size];
        unsafe {
            oidnReadBuffer(
                self.buf,
                0,
                self.size * mem::size_of::<f32>(),
                contents.as_mut_ptr() as *mut _,
            );
        }
        contents
    }

    /// # Safety
    /// Raw buffer must not be made invalid (e.g. by destroying it)
    pub unsafe fn raw(&self) -> OIDNBuffer {
        self.buf
    }

    /// # Safety
    /// You may not assign a new sync lock to this sync lock e.g.
    /// ````ignore
    /// // Undefined behaviour.
    /// *buffer.raw_sync_lock() = SyncLock::default();
    /// ````
    pub unsafe fn raw_sync_lock(&mut self) -> &mut SyncLock {
        &mut self.sync_lock
    }

    /// The size in number of f32s
    pub fn size(&self) -> usize {
        self.size
    }
}

impl Drop for Buffer {
    fn drop(&mut self) {
        unsafe {
            // It can't be guaranteed that the MustSync has already synchronised.
            oidnSyncDevice(self.device);
            // Decrements the ref-count of the device since we incremented it on creation.
            oidnRetainDevice(self.device);
            oidnReleaseBuffer(self.buf);
        }
    }
}

unsafe impl Send for Buffer {}
