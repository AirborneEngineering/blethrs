//! Items shared between `blethrs` (firmware) and `blethrs-link` (software).

#![no_std]

#[repr(u32)]
pub enum Command {
    Info = 0,
    Read = 1,
    Erase = 2,
    Write = 3,
    Boot = 4,
}

pub struct UnknownValue;

#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Success = 0,
    InvalidAddress = 1,
    LengthNotMultiple4 = 2,
    LengthTooLong = 3,
    DataLengthIncorrect = 4,
    EraseError = 5,
    WriteError = 6,
    FlashError = 7,
    NetworkError = 8,
    InternalError = 9,
}

pub const CONFIG_MAGIC: u32 = 0x67797870;

/// Start address of each sector in flash
pub const FLASH_SECTOR_ADDRESSES: [u32; 12] = [
    0x0800_0000, 0x0800_4000, 0x0800_8000, 0x0800_C000,
    0x0801_0000, 0x0802_0000, 0x0804_0000, 0x0806_0000,
    0x0808_0000, 0x080A_0000, 0x080C_0000, 0x080E_0000,
];
/// Final valid address in flash
pub const FLASH_END: u32 = 0x080F_FFFF;
/// Address of configuration sector. Must be one of the start addresses in FLASH_SECTOR_ADDRESSES.
pub const FLASH_CONFIG: u32 = FLASH_SECTOR_ADDRESSES[3];
/// Address of user firmware sector. Must be one of the start addresses in FLASH_SECTOR_ADDRESSES.
pub const FLASH_USER: u32 = FLASH_SECTOR_ADDRESSES[4];

impl core::convert::TryFrom<u32> for Command {
    type Error = UnknownValue;
    fn try_from(u: u32) -> Result<Self, Self::Error> {
        let cmd = match u {
            0 => Command::Info,
            1 => Command::Read,
            2 => Command::Erase,
            3 => Command::Write,
            4 => Command::Boot,
            _ => return Err(UnknownValue),
        };
        Ok(cmd)
    }
}

impl core::convert::TryFrom<u32> for Error {
    type Error = UnknownValue;
    fn try_from(u: u32) -> Result<Self, Self::Error> {
        let cmd = match u {
            0 => Error::Success,
            1 => Error::InvalidAddress,
            2 => Error::LengthNotMultiple4,
            3 => Error::LengthTooLong,
            4 => Error::DataLengthIncorrect,
            5 => Error::EraseError,
            6 => Error::WriteError,
            7 => Error::FlashError,
            8 => Error::NetworkError,
            9 => Error::InternalError,
            _ => return Err(UnknownValue),
        };
        Ok(cmd)
    }
}
