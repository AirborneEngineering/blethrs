use core;
use cortex_m;
use stm32f407;

static mut USER_RESET: Option<extern "C" fn()> = None;
static BOOTLOAD_FLAG: u32 = 0x2000_0000;

/// Returns true if the most recent reset was due to a software request
///
/// Clears the reset cause before returning.
fn was_software_reset(rcc: &mut stm32f407::RCC) -> bool {
    let result = rcc.csr.read().sftrstf().bit_is_set();
    rcc.csr.modify(|_, w| w.rmvf().set_bit());
    result
}

/// Returns true if the bootload flag is set: RAM 0x2000_0000 == 0xB00110AD
///
/// Clears the flag before returning.
fn flag_set() -> bool {
    let flag: u32 = unsafe { *(BOOTLOAD_FLAG as *const u32) };
    clear_flag();
    return flag == 0xB00110AD;
}

fn clear_flag() {
    unsafe { *(BOOTLOAD_FLAG as *mut u32) = 0 }
}

/// Return true if we should try to bootload the user code
pub fn should_bootload(rcc: &mut stm32f407::RCC) -> bool {
    !(was_software_reset(rcc) && flag_set())
}

/// Trigger a reset that will cause us to bootload the user application next go around
pub fn reset_bootload() {
    clear_flag();
    // This is SCB.AIRCR, but it's a pain to require SCB be passed in to this function.
    unsafe { *(0xE000ED0C as *mut u32) = (0x5FA<<16) | (1<<2) };
}

/// Jump to user code at the given address.
///
/// Doesn't disable interrupts so only call this right at boot,
/// when no interrupt sources will be enabled.
pub fn bootload(scb: &mut cortex_m::peripheral::SCB, address: u32) {
    unsafe {
        let sp = *(address as *const u32);
        let rv = *((address + 4) as *const u32);

        USER_RESET = Some(core::mem::transmute(rv));
        scb.vtor.write(address);
        cortex_m::register::msp::write(sp);
        (USER_RESET.unwrap())();
    }
}
