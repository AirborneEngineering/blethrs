#![no_std]

pub mod bootload;
pub mod cmd;
pub mod flash;
#[cfg(feature = "stm32f107")]
pub mod stm32f107;
#[cfg(feature = "stm32f407")]
pub mod stm32f407;
#[cfg(feature = "ufmt")]
mod ufmt;

#[cfg(feature = "stm32f107")]
pub use stm32f107 as stm32;
#[cfg(feature = "stm32f407")]
pub use stm32f407 as stm32;

pub use blethrs_shared::Error;

pub type Result<T> = core::result::Result<T, Error>;
