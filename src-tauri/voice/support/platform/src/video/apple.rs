//! VideoToolbox helpers for live decoding (live_apple.rs): the session wrapper, its creation and
//! the NV12 copy-out. Trimmed from Serein's `platform::video::apple` (its attachment decoder, MP4
//! demuxer and AAC decoding are left out). MIT OR Apache-2.0, from Serein
//! (https://github.com/ViceVerse-cz/Serein).
#![allow(unsafe_code)]

use super::{Color, INVALID, Layout, Matrix, Picture, UNSUPPORTED, pack_plane};
use objc2_core_foundation::{CFArray, CFDictionary, CFNumber, CFRetained, CFString};
use objc2_core_media::{CMFormatDescription, CMTime};
use objc2_core_video::{
	CVImageBuffer, CVPixelBufferGetBaseAddressOfPlane, CVPixelBufferGetBytesPerRowOfPlane,
	CVPixelBufferGetHeight, CVPixelBufferGetHeightOfPlane, CVPixelBufferGetPixelFormatType,
	CVPixelBufferGetPlaneCount, CVPixelBufferGetWidth, CVPixelBufferGetWidthOfPlane,
	CVPixelBufferLockBaseAddress, CVPixelBufferLockFlags, CVPixelBufferUnlockBaseAddress,
	kCVImageBufferYCbCrMatrix_ITU_R_601_4, kCVImageBufferYCbCrMatrix_ITU_R_709_2,
	kCVImageBufferYCbCrMatrix_ITU_R_2020, kCVImageBufferYCbCrMatrix_SMPTE_240M_1995,
	kCVImageBufferYCbCrMatrixKey, kCVPixelBufferPixelFormatTypeKey,
	kCVPixelFormatType_420YpCbCr8BiPlanarFullRange,
	kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange,
};
use objc2_video_toolbox::{VTDecodeInfoFlags, VTDecompressionOutputCallbackRecord, VTDecompressionSession};
use std::{
	ffi::c_void,
	ptr::{NonNull, null_mut},
};

/// Largest single decoded frame accepted.
const MAX_BYTES: usize = 16 * 1024 * 1024;

/// The decoders' native bi-planar 4:2:0 outputs; VideoToolbox picks the stream's own range,
/// so no pixel transfer runs before the renderer converts colour.
pub(super) const NV12_FORMATS: [u32; 2] = [
	kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange,
	kCVPixelFormatType_420YpCbCr8BiPlanarFullRange,
];

pub(super) struct Session(pub(super) CFRetained<VTDecompressionSession>);
impl Drop for Session {
	fn drop(&mut self) {
		// SAFETY: Invalidate stops callbacks before the shared output slot is released.
		unsafe { self.0.invalidate() };
	}
}

pub(super) fn create_session(
	format: &CMFormatDescription,
	refcon: *mut c_void,
	callback: unsafe extern "C-unwind" fn(
		*mut c_void,
		*mut c_void,
		i32,
		VTDecodeInfoFlags,
		*mut CVImageBuffer,
		CMTime,
		CMTime,
	),
	pixel_formats: &[u32],
	decoder_specification: Option<&CFDictionary>,
) -> Result<Session, &'static str> {
	let numbers: Vec<_> = pixel_formats
		.iter()
		.map(|format| CFNumber::new_i32(*format as i32))
		.collect();
	let formats = CFArray::from_retained_objects(&numbers);
	// SAFETY: The static key is a valid CFString; the dictionary is a plain attribute map.
	let attributes =
		unsafe { CFDictionary::from_slices(&[kCVPixelBufferPixelFormatTypeKey], &[&*formats]) };
	let record = VTDecompressionOutputCallbackRecord {
		decompressionOutputCallback: Some(callback),
		decompressionOutputRefCon: refcon,
	};
	let mut session: *mut VTDecompressionSession = null_mut();
	// SAFETY: The callback record and attribute dictionary outlive the call; `refcon` points
	// at the decoder's `Arc<Mutex<Output>>`, which outlives the session (see field order).
	let status = unsafe {
		VTDecompressionSession::create(
			None,
			format,
			decoder_specification,
			Some(attributes.as_opaque()),
			&record,
			NonNull::from(&mut session),
		)
	};
	if status != 0 {
		return Err(UNSUPPORTED);
	}
	// SAFETY: Create returns a +1 reference on success.
	NonNull::new(session)
		.map(|ptr| Session(unsafe { CFRetained::from_raw(ptr) }))
		.ok_or(UNSUPPORTED)
}

/// Copy one bi-planar 4:2:0 picture into tightly packed NV12 after checking every bound.
pub(super) unsafe fn copy_nv12(pixels: &CVImageBuffer) -> Result<Picture, &'static str> {
	let width = CVPixelBufferGetWidth(pixels);
	let height = CVPixelBufferGetHeight(pixels);
	let (w, h) = (
		u32::try_from(width).map_err(|_| INVALID)?,
		u32::try_from(height).map_err(|_| INVALID)?,
	);
	super::check_dimensions(w, h)?;
	let format = CVPixelBufferGetPixelFormatType(pixels);
	if !NV12_FORMATS.contains(&format) {
		return Err(INVALID);
	}
	let full_range = format == kCVPixelFormatType_420YpCbCr8BiPlanarFullRange;
	let (chroma_width, chroma_height) = (width.div_ceil(2), height.div_ceil(2));
	if CVPixelBufferGetPlaneCount(pixels) != 2
		|| CVPixelBufferGetWidthOfPlane(pixels, 0) != width
		|| CVPixelBufferGetHeightOfPlane(pixels, 0) != height
		|| CVPixelBufferGetWidthOfPlane(pixels, 1) != chroma_width
		|| CVPixelBufferGetHeightOfPlane(pixels, 1) != chroma_height
	{
		return Err(INVALID);
	}
	// SAFETY: The static keys are valid CFStrings and the attachment is a retained copy.
	let matrix = unsafe {
		pixels
			.attachment(kCVImageBufferYCbCrMatrixKey, null_mut())
			.and_then(|value| {
				let value = value.downcast_ref::<CFString>()?;
				[
					(kCVImageBufferYCbCrMatrix_ITU_R_709_2, Matrix::Bt709),
					(kCVImageBufferYCbCrMatrix_ITU_R_601_4, Matrix::Bt601),
					(kCVImageBufferYCbCrMatrix_ITU_R_2020, Matrix::Bt2020),
					(kCVImageBufferYCbCrMatrix_SMPTE_240M_1995, Matrix::Smpte240m),
				]
				.into_iter()
				.find_map(|(tag, matrix)| (value == tag).then_some(matrix))
			})
	};
	let color = match matrix {
		Some(matrix) => Color { matrix, full_range },
		None => Color::untagged(h, full_range),
	};
	let len = Layout::Nv12.len(w, h).ok_or(INVALID)?;
	if len > MAX_BYTES {
		return Err(INVALID);
	}
	// SAFETY: The buffer is locked read-only for the copy and always unlocked afterwards;
	// each plane is read only within its own reported stride and row count.
	let mut data = Vec::with_capacity(len);
	unsafe {
		if CVPixelBufferLockBaseAddress(pixels, CVPixelBufferLockFlags::ReadOnly) != 0 {
			return Err(INVALID);
		}
		let mut copied = Some(());
		for (plane, row_bytes, rows) in [(0, width, height), (1, chroma_width * 2, chroma_height)] {
			let base = CVPixelBufferGetBaseAddressOfPlane(pixels, plane);
			let stride = CVPixelBufferGetBytesPerRowOfPlane(pixels, plane);
			copied = copied.and_then(|()| {
				let total = stride
					.checked_mul(rows)
					.filter(|total| *total <= MAX_BYTES)?;
				(!base.is_null()).then_some(())?;
				let source = std::slice::from_raw_parts(base.cast::<u8>(), total);
				pack_plane(&mut data, source, stride, row_bytes, rows)
			});
		}
		CVPixelBufferUnlockBaseAddress(pixels, CVPixelBufferLockFlags::ReadOnly);
		copied.ok_or(INVALID)?;
	}
	let picture = Picture {
		width: w,
		height: h,
		layout: Layout::Nv12,
		color,
		rotation: 0,
		data,
	};
	picture.is_valid().then_some(picture).ok_or(INVALID)
}
