//! asdf

#![cfg_attr(not(feature = "std"), no_std)]

pub mod cpu_arch;
pub mod cpu_fam;
#[cfg(feature = "std")]
pub mod os;
