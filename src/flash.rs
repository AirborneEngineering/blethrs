use stm32f407;


const FLASH_SECTOR_ADDRESSES: [u32; 12] =
    [0x0800_0000, 0x0800_4000, 0x0800_8000, 0x0800_C000,
     0x0801_0000, 0x0802_0000, 0x0804_0000, 0x0806_0000,
     0x0808_0000, 0x080A_0000, 0x080C_0000, 0x080E_0000];
const FLASH_END: u32 = 0x080F_FFFF;

const FLASH_SECTOR_CONFIG: u32 = FLASH_SECTOR_ADDRESSES[3];
const FLASH_SECTOR_USER: u32   = FLASH_SECTOR_ADDRESSES[4];


#[derive(Copy,Clone)]
#[repr(C,packed)]
pub struct UserConfig {
    pub mac_address: [u8; 6],
    pub ip_address: [u8; 4],
    _padding: [u8; 6],
    checksum: u32,
}

static DEFAULT_CONFIG: UserConfig = UserConfig {
    // Locally administered MAC
    mac_address: [0x02, 0x00, 0x01, 0x02, 0x03, 0x04],
    ip_address: [10, 1, 1, 10],
    _padding: [0u8; 6],
    checksum: 0,
};

impl UserConfig {
    /// Attempt to read the UserConfig from flash sector 3 at 0x0800_C000.
    /// If a valid config cannot be read, the default one is returned instead.
    pub fn get(crc: &mut stm32f407::CRC) -> UserConfig {
        let raw: &[u32; 5] = unsafe { *(FLASH_SECTOR_CONFIG as *const _) };
        crc.cr.write(|w| w.reset().reset());
        for idx in 0..(raw.len() - 1) {
            crc.dr.write(|w| w.dr().bits(raw[idx]));
        }
        let crc_computed = crc.dr.read().dr().bits();
        let crc_stored = raw[raw.len() - 1];
        if crc_computed == crc_stored {
            let ptr = unsafe { *(FLASH_SECTOR_CONFIG as *const UserConfig) };
            ptr.clone()
        } else {
            DEFAULT_CONFIG.clone()
        }
    }
}

/// Try to determine if there is valid code in the user flash at 0x0801_0000.
/// Returns Some(u32) with the address to jump to if so, and None if not.
pub fn valid_user_code() -> Option<u32> {
    let reset_vector: u32 = unsafe { *((FLASH_SECTOR_USER + 4) as *const u32) };
    if reset_vector >= FLASH_SECTOR_USER && reset_vector <= FLASH_END {
        Some(FLASH_SECTOR_USER)
    } else {
        None
    }
}
