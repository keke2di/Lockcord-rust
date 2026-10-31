//! The Media Foundation runtime and DXGI device manager from Serein's `media_foundation.rs` (its
//! attachment decoder is left out): what the live H.264 decoder needs. MIT OR Apache-2.0.
#![allow(unsafe_code)]

use std::{marker::PhantomData, rc::Rc};
use windows::{
	Win32::{
		Foundation::HMODULE,
		Graphics::{
			Direct3D::D3D_DRIVER_TYPE_HARDWARE,
			Direct3D11::{D3D11_CREATE_DEVICE_VIDEO_SUPPORT, D3D11_SDK_VERSION, D3D11CreateDevice, ID3D11Device, ID3D11Multithread},
		},
		Media::MediaFoundation::*,
		System::Com::{COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize},
	},
	core::Interface,
};

use super::UNSUPPORTED;


/// A D3D11 video device wrapped in a DXGI device manager, the handle Media Foundation needs
/// for DXVA decoding. `None` leaves the caller in software mode.
pub(super) unsafe fn dxgi_manager() -> Option<(ID3D11Device, IMFDXGIDeviceManager)> {
	unsafe {
		let mut device: Option<ID3D11Device> = None;
		D3D11CreateDevice(
			None,
			D3D_DRIVER_TYPE_HARDWARE,
			HMODULE::default(),
			D3D11_CREATE_DEVICE_VIDEO_SUPPORT,
			None,
			D3D11_SDK_VERSION,
			Some(&mut device),
			None,
			None,
		)
		.ok()?;
		let device = device?;
		if let Ok(multithread) = device.cast::<ID3D11Multithread>() {
			let _ = multithread.SetMultithreadProtected(true);
		}
		let mut token = 0;
		let mut manager: Option<IMFDXGIDeviceManager> = None;
		MFCreateDXGIDeviceManager(&mut token, &mut manager).ok()?;
		let manager = manager?;
		manager.ResetDevice(&device, token).ok()?;
		Some((device, manager))
	}
}

pub(super) struct Runtime(PhantomData<Rc<()>>);
impl Runtime {
	pub(super) fn open() -> Result<Self, &'static str> {
		// SAFETY: Called and dropped only on the dedicated decoder thread.
		unsafe {
			CoInitializeEx(None, COINIT_MULTITHREADED)
				.ok()
				.map_err(|_| UNSUPPORTED)?;
			if MFStartup(MF_VERSION, MFSTARTUP_NOSOCKET).is_err() {
				CoUninitialize();
				return Err(UNSUPPORTED);
			}
		}
		Ok(Self(PhantomData))
	}
}
impl Drop for Runtime {
	fn drop(&mut self) {
		// SAFETY: Balanced successful initialization on this same thread, after COM fields drop.
		unsafe {
			let _ = MFShutdown();
			CoUninitialize();
		}
	}
}
