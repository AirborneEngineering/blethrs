#![no_std]

pub mod bootload;
pub mod cmd;
pub mod flash;
#[cfg(feature = "ufmt")]
mod ufmt;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Error {
    Success,
    InvalidAddress,
    LengthNotMultiple4,
    LengthTooLong,
    DataLengthIncorrect,
    EraseError,
    WriteError,
    FlashError,
    NetworkError,
    InternalError,
}

pub type Result<T> = core::result::Result<T, Error>;
