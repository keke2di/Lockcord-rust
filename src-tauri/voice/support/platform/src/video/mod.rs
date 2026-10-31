//! Live H.264 decoding (camera and Go Live) from Serein's `platform::video`, Windows only:
//! `picture.rs` and `live_windows.rs` copied unchanged; the attachment decoders are left out.
//! MIT OR Apache-2.0, from Serein (https://github.com/ViceVerse-cz/Serein).

mod picture;
pub use picture::{Color, Layout, Matrix, Picture, pack_plane};

mod media_foundation;

/// Hardware-backed decoding of one live H.264 elementary stream: the Media Foundation decoder
/// with a DXGI device manager when available.
#[path = "live_windows.rs"]
pub mod live;

/// Largest live access unit any backend accepts.
pub const MAX_ACCESS_UNIT: usize = 2 * 1024 * 1024 + 64 * 1024;

/// One decoded live picture.
pub type LiveFrame = Picture;

/// Receives live pictures from the decoder's own thread, in decode order.
pub type LiveSink = Box<dyn Fn(LiveFrame) + Send + Sync>;

pub const UNSUPPORTED: &str = "This video format or codec is not supported on this system.";
pub const INVALID: &str = "The video could not be decoded safely.";
/// A live decoder queue is full; the access unit was dropped but the decoder is healthy.
pub const BUSY: &str = "The video decoder is falling behind.";
pub const TOO_LARGE: &str = "Video up to 1080p is supported.";

/// 1080p worth of pixels within a 1920 px square.
pub fn check_dimensions(width: u32, height: u32) -> Result<(), &'static str> {
	if width == 0 || height == 0 {
		return Err(INVALID);
	}
	if width > 1920 || height > 1920 || u64::from(width) * u64::from(height) > 1920 * 1080 {
		return Err(TOO_LARGE);
	}
	Ok(())
}

/// Rotate a packed RGBA frame clockwise by a quarter-turn multiple; returns the new dimensions.
pub(crate) fn rotate_rgba(rgba: &[u8], width: u32, height: u32, rotation: u32) -> (u32, u32, Vec<u8>) {
	let (w, h) = (width as usize, height as usize);
	if rotation == 0 || rgba.len() != w * h * 4 {
		return (width, height, rgba.to_vec());
	}
	let (out_w, out_h) = if rotation == 90 || rotation == 270 { (h, w) } else { (w, h) };
	let mut out = vec![0; rgba.len()];
	for y in 0..h {
		for x in 0..w {
			let (dx, dy) = match rotation {
				90 => (h - 1 - y, x),
				180 => (w - 1 - x, h - 1 - y),
				270 => (y, w - 1 - x),
				_ => (x, y),
			};
			let source = (y * w + x) * 4;
			let target = (dy * out_w + dx) * 4;
			out[target..target + 4].copy_from_slice(&rgba[source..source + 4]);
		}
	}
	(out_w as u32, out_h as u32, out)
}
