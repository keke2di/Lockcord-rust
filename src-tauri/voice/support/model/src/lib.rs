//! The two pieces of Serein's `model` crate its voice crate uses: Discord ids and the
//! microphone-processing settings (`voice_settings.rs`, copied unchanged). MIT OR Apache-2.0,
//! from Serein (https://github.com/ViceVerse-cz/Serein); see ../../LICENSE-MIT.
use std::{fmt, str::FromStr};

pub mod voice_settings;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Id(pub u64);
impl fmt::Display for Id {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		self.0.fmt(f)
	}
}
impl FromStr for Id {
	type Err = &'static str;
	fn from_str(s: &str) -> Result<Self, Self::Err> {
		if s.is_empty() || s.len() > 20 || !s.bytes().all(|b| b.is_ascii_digit()) {
			return Err("Invalid Discord ID");
		}
		s.parse::<u64>().ok().filter(|v| *v != 0).map(Self).ok_or("Invalid Discord ID")
	}
}
