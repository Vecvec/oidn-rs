use crate::sys::{
    oidnGetBufferSize, oidnNewBuffer, oidnReadBuffer, oidnReleaseBuffer, oidnWriteBuffer,
    OIDNBuffer,
};
use crate::Device;
use std::mem;

pub struct Buffer {
    pub(crate) buf: OIDNBuffer,
    pub(crate) size: usize,
    pub(crate) id: isize,
}

impl Device {
    /// Creates a new buffer from a slice, returns null if buffer creation failed
    pub fn create_buffer(&self, contents: &[f32]) -> Option<Buffer> {
        let byte_size = mem::size_of_val(contents);
        let buffer = unsafe {
            let buf = oidnNewBuffer(self.0, byte_size);
            if buf.is_null() {
                return None;
            }
            oidnWriteBuffer(buf, 0, byte_size, contents.as_ptr() as *const _);
            buf
        };
        Some(Buffer {
            buf: buffer,
            size: contents.len(),
            id: self.0 as isize,
        })
    }
    /// # Safety
    /// Raw buffer must not be invalid (e.g. destroyed, null ect.)
    ///
    /// Raw buffer must have been created by this device
    pub unsafe fn create_buffer_from_raw(&self, buffer: OIDNBuffer) -> Buffer {
        let size = oidnGetBufferSize(buffer);
        Buffer {
            buf: buffer,
            size,
            id: self.0 as isize,
        }
    }
}

impl Buffer {
    /// Writes to the buffer, returns [None] if the sizes mismatch
    pub fn write(&mut self, contents: &[f32]) -> Option<()> {
        if self.size != contents.len() {
            return None;
        }
        let byte_size = mem::size_of_val(contents);
        unsafe {
            oidnWriteBuffer(self.buf, 0, byte_size, contents.as_ptr() as *const _);
        }
        Some(())
    }
    /// Reads from the buffer to the array, returns [None] if the sizes mismatch
    pub fn read_to_slice(&mut self, contents: &mut [f32]) -> Option<()> {
        if self.size != contents.len() {
            return None;
        }
        let byte_size = mem::size_of_val(contents);
        unsafe {
            oidnReadBuffer(self.buf, 0, byte_size, contents.as_ptr() as *mut _);
        }
        Some(())
    }
    /// Reads from the buffer
    pub fn read(&mut self) -> Vec<f32> {
        let contents = vec![0.0; self.size * mem::size_of::<f32>()];
        unsafe {
            oidnReadBuffer(
                self.buf,
                0,
                self.size * mem::size_of::<f32>(),
                contents.as_ptr() as *mut _,
            );
        }
        contents
    }
    /// # Safety
    /// Raw buffer must not be made invalid (e.g. by destroying it)
    pub unsafe fn raw(&self) -> OIDNBuffer {
        self.buf
    }
    pub fn size(&self) -> usize {
        self.size
    }
}

impl Drop for Buffer {
    fn drop(&mut self) {
        unsafe { oidnReleaseBuffer(self.buf) }
    }
}
