use crate::{Error, Quality, buffer::Buffer, device::Device, sys::*};
use std::mem;

enum BufferOrBufferRef<'a> {
    Buffer(Buffer),
    BufferRef(&'a Buffer),
}

impl BufferOrBufferRef<'_> {
    fn buffer_ref(&self) -> &Buffer {
        match self {
            Self::Buffer(buf) => buf,
            Self::BufferRef(buf) => buf,
        }
    }
}

/// A generic ray tracing denoising filter for denoising
/// images produces with Monte Carlo ray tracing methods
/// such as path tracing.
pub struct RayTracing<'a, 'b> {
    handle: OIDNFilter,
    device: &'a Device,
    albedo: Option<BufferOrBufferRef<'b>>,
    normal: Option<BufferOrBufferRef<'b>>,
    hdr: bool,
    input_scale: f32,
    srgb: bool,
    clean_aux: bool,
    img_dims: (usize, usize, usize),
    filter_quality: OIDNQuality,
}

fn upload_or_create_new(buffer: &mut Option<BufferOrBufferRef<'_>>, data: &[f32], device: &Device) {
    match buffer.as_mut().and_then(|buf| {
        // No BufferRef, as we don't want to upload to a user's buffer.
        if let BufferOrBufferRef::Buffer(buf) = buf {
            if buf.size == data.len() {
                Some(buf)
            } else {
                None
            }
        } else {
            None
        }
    }) {
        None => {
            *buffer = Some(BufferOrBufferRef::Buffer(
                device.create_buffer(data).unwrap(),
            ));
        }
        Some(buf) => {
            buf.write(data)
                .expect("we check if the size is the same already");
        }
    }
}

impl<'a, 'b> RayTracing<'a, 'b> {
    pub fn new(device: &'a Device) -> RayTracing<'a, 'b> {
        unsafe {
            oidnRetainDevice(device.0);
        }
        let filter = unsafe { oidnNewFilter(device.0, b"RT\0" as *const _ as _) };
        RayTracing {
            handle: filter,
            device,
            albedo: None,
            normal: None,
            hdr: false,
            input_scale: f32::NAN,
            srgb: false,
            clean_aux: false,
            img_dims: (0, 0, 0),
            filter_quality: 0,
        }
    }

    /// Sets the quality of the output, the default is high.
    ///
    /// Balanced lowers the precision, if possible, however
    /// some devices will not support this and so
    /// the result (and performance) will stay the same as high.
    /// Balanced is recommended for realtime usages.
    pub fn filter_quality(&mut self, quality: Quality) -> &mut RayTracing<'a, 'b> {
        self.filter_quality = quality.as_raw_oidn_quality();
        self
    }

    /// Set input auxiliary images containing the albedo and normals.
    ///
    /// Albedo must have three channels per pixel with values in [0, 1].
    /// Normal must contain the shading normal as three channels per pixel
    /// *world-space* or *view-space* vectors with arbitrary length, values
    /// in `[-1, 1]`.
    ///
    /// # Panics
    /// - if resource creation fails
    pub fn albedo_normal(&mut self, albedo: &[f32], normal: &[f32]) -> &mut RayTracing<'a, 'b> {
        upload_or_create_new(&mut self.albedo, albedo, self.device);
        upload_or_create_new(&mut self.normal, normal, self.device);
        self
    }

    /// Set an input auxiliary image containing the albedo per pixel (three
    /// channels, values in `[0, 1]`).
    ///
    /// # Panics
    /// - if resource creation fails
    pub fn albedo(&mut self, albedo: &[f32]) -> &mut RayTracing<'a, 'b> {
        upload_or_create_new(&mut self.albedo, albedo, self.device);
        self
    }
    /// Set input auxiliary buffer containing the albedo and normals.
    ///
    /// Albedo buffer must have three channels per pixel with values in [0, 1].
    /// Normal must contain the shading normal as three channels per pixel
    /// *world-space* or *view-space* vectors with arbitrary length, values
    /// in `[-1, 1]`.
    ///
    /// This function is the same as [RayTracing::albedo_normal] but takes
    /// buffers instead
    ///
    /// Returns [None] if either buffer was not created by this device
    pub fn albedo_normal_buffer(
        &mut self,
        albedo: &'b Buffer,
        normal: &'b Buffer,
    ) -> Option<&mut RayTracing<'a, 'b>> {
        if !self.device.same_device_as_buf(albedo) || !self.device.same_device_as_buf(normal) {
            return None;
        }
        self.albedo = Some(BufferOrBufferRef::BufferRef(albedo));
        self.normal = Some(BufferOrBufferRef::BufferRef(normal));
        Some(self)
    }

    /// Set an input auxiliary buffer containing the albedo per pixel (three
    /// channels, values in `[0, 1]`).
    ///
    /// This function is the same as [RayTracing::albedo] but takes buffers
    /// instead
    ///
    /// Returns [None] if albedo buffer was not created by this device
    pub fn albedo_buffer(&mut self, albedo: &'b Buffer) -> Option<&mut RayTracing<'a, 'b>> {
        if !self.device.same_device_as_buf(albedo) {
            return None;
        }
        self.albedo = Some(BufferOrBufferRef::BufferRef(albedo));
        Some(self)
    }

    /// Set whether the color is HDR.
    pub fn hdr(&mut self, hdr: bool) -> &mut RayTracing<'a, 'b> {
        self.hdr = hdr;
        self
    }

    #[deprecated(since = "1.3.1", note = "Please use RayTracing::input_scale instead")]
    pub fn hdr_scale(&mut self, hdr_scale: f32) -> &mut RayTracing<'a, 'b> {
        self.input_scale = hdr_scale;
        self
    }

    /// Sets a scale to apply to input values before filtering, without scaling
    /// the output too.
    ///
    /// This can be used to map color or auxiliary feature values to the
    /// expected range. E.g. for mapping HDR values to physical units (which
    /// affects the quality of the output but not the range of the output
    /// values). If not set, the scale is computed implicitly for HDR images
    /// or set to 1 otherwise
    pub fn input_scale(&mut self, input_scale: f32) -> &mut RayTracing<'a, 'b> {
        self.input_scale = input_scale;
        self
    }

    /// Set whether the color is encoded with the sRGB (or 2.2 gamma) curve (LDR
    /// only) or is linear.
    ///
    /// The output will be encoded with the same curve.
    pub fn srgb(&mut self, srgb: bool) -> &mut RayTracing<'a, 'b> {
        self.srgb = srgb;
        self
    }

    /// Set whether the auxiliary feature (albedo, normal) images are
    /// noise-free.
    ///
    /// Recommended for highest quality but should not be enabled for noisy
    /// auxiliary images to avoid residual noise.
    pub fn clean_aux(&mut self, clean_aux: bool) -> &mut RayTracing<'a, 'b> {
        self.clean_aux = clean_aux;
        self
    }

    /// sets the dimensions of the denoising image, if new width * new height
    /// does not equal old width * old height
    pub fn image_dimensions(&mut self, width: usize, height: usize) -> &mut RayTracing<'a, 'b> {
        let buffer_dims = 3 * width * height;
        match &self.albedo {
            None => {}
            Some(buffer) => {
                if buffer.buffer_ref().size != buffer_dims {
                    self.albedo = None;
                }
            }
        }
        match &self.normal {
            None => {}
            Some(buffer) => {
                if buffer.buffer_ref().size != buffer_dims {
                    self.normal = None;
                }
            }
        }
        self.img_dims = (width, height, buffer_dims);
        self
    }

    pub fn filter(&self, color: &[f32], output: &mut [f32]) -> Result<(), Error> {
        self.execute_filter(Some(color), output)
    }

    pub fn filter_buffer(&self, color: &Buffer, output: &Buffer) -> Result<(), Error> {
        self.execute_filter_buffer(Some(color), output)
    }

    pub fn filter_in_place(&self, color: &mut [f32]) -> Result<(), Error> {
        self.execute_filter(None, color)
    }

    pub fn filter_in_place_buffer(&self, color: &Buffer) -> Result<(), Error> {
        self.execute_filter_buffer(None, color)
    }

    fn execute_filter(&self, color: Option<&[f32]>, output: &mut [f32]) -> Result<(), Error> {
        let color = match color {
            None => None,
            Some(color) => Some(self.device.create_buffer(color).ok_or(Error::OutOfMemory)?),
        };
        let out = self
            .device
            .create_buffer(output)
            .ok_or(Error::OutOfMemory)?;
        self.execute_filter_buffer(color.as_ref(), &out)?;
        unsafe {
            oidnReadBuffer(
                out.buf,
                0,
                out.size * mem::size_of::<f32>(),
                output.as_mut_ptr() as *mut _,
            )
        };
        Ok(())
    }

    fn execute_filter_buffer(&self, color: Option<&Buffer>, output: &Buffer) -> Result<(), Error> {
        if let Some(alb) = &self.albedo {
            if alb.buffer_ref().size != self.img_dims.2 {
                return Err(Error::InvalidImageDimensions);
            }
            unsafe {
                oidnSetFilterImage(
                    self.handle,
                    b"albedo\0" as *const _ as _,
                    alb.buffer_ref().buf,
                    OIDNFormat_OIDN_FORMAT_FLOAT3,
                    self.img_dims.0 as _,
                    self.img_dims.1 as _,
                    0,
                    0,
                    0,
                );
            }

            // No use supplying normal if albedo was
            // not also given.
            if let Some(norm) = &self.normal {
                if norm.buffer_ref().size != self.img_dims.2 {
                    return Err(Error::InvalidImageDimensions);
                }
                unsafe {
                    oidnSetFilterImage(
                        self.handle,
                        b"normal\0" as *const _ as _,
                        norm.buffer_ref().buf,
                        OIDNFormat_OIDN_FORMAT_FLOAT3,
                        self.img_dims.0 as _,
                        self.img_dims.1 as _,
                        0,
                        0,
                        0,
                    );
                }
            }
        }
        let color_buffer = match color {
            Some(color) => {
                if !self.device.same_device_as_buf(color) {
                    return Err(Error::InvalidArgument);
                }
                if color.size != self.img_dims.2 {
                    return Err(Error::InvalidImageDimensions);
                }
                color
            }
            None => {
                if output.size != self.img_dims.2 {
                    return Err(Error::InvalidImageDimensions);
                }
                // actually this is a needed borrow, the compiler complains otherwise
                #[allow(clippy::needless_borrow)]
                &output
            }
        };
        unsafe {
            oidnSetFilterImage(
                self.handle,
                b"color\0" as *const _ as _,
                color_buffer.buf,
                OIDNFormat_OIDN_FORMAT_FLOAT3,
                self.img_dims.0 as _,
                self.img_dims.1 as _,
                0,
                0,
                0,
            );
        }
        if !self.device.same_device_as_buf(output) {
            return Err(Error::InvalidArgument);
        }
        if output.size != self.img_dims.2 {
            return Err(Error::InvalidImageDimensions);
        }
        unsafe {
            oidnSetFilterImage(
                self.handle,
                b"output\0" as *const _ as _,
                output.buf,
                OIDNFormat_OIDN_FORMAT_FLOAT3,
                self.img_dims.0 as _,
                self.img_dims.1 as _,
                0,
                0,
                0,
            );
            oidnSetFilterBool(self.handle, b"hdr\0" as *const _ as _, self.hdr);
            oidnSetFilterFloat(
                self.handle,
                b"inputScale\0" as *const _ as _,
                self.input_scale,
            );
            oidnSetFilterBool(self.handle, b"srgb\0" as *const _ as _, self.srgb);
            oidnSetFilterBool(self.handle, b"clean_aux\0" as *const _ as _, self.clean_aux);

            oidnSetFilterInt(
                self.handle,
                b"quality\0" as *const _ as _,
                self.filter_quality as i32,
            );

            oidnCommitFilter(self.handle);
            oidnExecuteFilter(self.handle);
        }
        Ok(())
    }
}

impl Drop for RayTracing<'_, '_> {
    fn drop(&mut self) {
        unsafe {
            oidnReleaseFilter(self.handle);
            oidnReleaseDevice(self.device.0);
        }
    }
}

unsafe impl Send for RayTracing<'_, '_> {}
