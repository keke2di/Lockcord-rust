//! The pieces of Serein's `client-core` crate its voice crate uses: one call's credentials and
//! the screen-share settings (`screen.rs`, copied unchanged). MIT OR Apache-2.0, from Serein
//! (https://github.com/ViceVerse-cz/Serein); see ../../LICENSE-MIT.

pub mod screen;

pub mod auth {
	#[derive(Clone, Copy, Debug, PartialEq, Eq)]
	pub enum Failure {
		Protocol,
	}
}

pub mod voice {
	use crate::auth::Failure;
	use model::Id;
	use std::fmt;
	use zeroize::Zeroizing;

	pub const MAX_PARTICIPANTS: usize = 64;

	/// A voice session id or token: bounded, never printed, wiped when dropped.
	pub struct Secret(Zeroizing<String>);
	impl Secret {
		pub fn new(value: String) -> Result<Self, Failure> {
			let value = Zeroizing::new(value);
			if value.is_empty() || value.len() > 2048 || !value.bytes().all(|b| b.is_ascii_graphic()) {
				return Err(Failure::Protocol);
			}
			Ok(Self(value))
		}
		pub fn expose(&self) -> &str {
			&self.0
		}
		#[allow(dead_code)]
		pub(crate) fn bytes(&self) -> usize {
			self.0.capacity()
		}
	}
	impl fmt::Debug for Secret {
		fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
			f.write_str("VoiceSecret([REDACTED])")
		}
	}

	/// What the voice server needs for one call (or one Go Live stream).
	pub struct VoiceConnection {
		pub channel: Id,
		pub guild: Option<Id>,
		pub user: Id,
		/// Expected peer for a one-to-one DM; groups use the authenticated voice roster.
		pub peer: Option<Id>,
		pub session: Secret,
		pub token: Secret,
		pub endpoint: String,
		pub request: u64,
	}
}
