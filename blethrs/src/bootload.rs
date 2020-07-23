use core;
use cortex_m;
use stm32f4xx_hal::stm32 as stm32f407;

/// Magic value used in this module to check if bootloader should start.
pub const BOOTLOAD_FLAG_VALUE: u32 = 0xB00110AD;
/// Address of magic value used in this module to check if bootloader should start.
pub const BOOTLOAD_FLAG_ADDRESS: u32 = 0x2000_0000;

static mut USER_RESET: Option<extern "C" fn()> = None;

/// This function should return true if the bootloader should enter bootload mode,
/// or false to immediately chainload the user firmware.
///
/// By default we check if there was a software reset and a magic value is set in RAM,
/// but you could also check GPIOs etc here.
///
/// Ensure any state change to the peripherals is reset before returning from this function.
pub fn should_enter(rcc: &mut stm32f407::RCC) -> bool {
    // Our plan is:
    // * If the reset was a software reset, and the magic flag is in the magic location,
    //   then the user firmware requested bootload, so enter bootload.
    // * Otherwise we check if PD2 is LOW for at least a full byte period of the UART,
    //   indicating someone has connected 3V to the external connector.
    let was_sw_reset = was_software_reset(rcc);
    let cond1 = was_sw_reset && flag_set();
    cond1
}

/// Returns true if the most recent reset was due to a software request
///
/// Clears the reset cause before returning, so this answer is only valid once.
pub fn was_software_reset(rcc: &mut stm32f407::RCC) -> bool {
    let result = rcc.csr.read().sftrstf().bit_is_set();
    rcc.csr.modify(|_, w| w.rmvf().set_bit());
    result
}

/// Returns true if the bootload flag is set: RAM 0x2000_0000 == 0xB00110AD
///
/// Clears the flag before returning, so this answer is only valid once.
pub fn flag_set() -> bool {
    cortex_m::interrupt::free(|_| unsafe {
        let flag = core::ptr::read_volatile(BOOTLOAD_FLAG_ADDRESS as *const u32);
        clear_flag();
        flag == BOOTLOAD_FLAG_VALUE
    })
}

fn clear_flag() {
    cortex_m::interrupt::free(|_| unsafe {
        core::ptr::write_volatile(BOOTLOAD_FLAG_ADDRESS as *mut u32, 0);
    });
}

/// Trigger a reset that will cause us to bootload the user application next go around
pub fn reset() {
    clear_flag();
    // It's troublesome to require SCB be passed in here, and
    // we're literally about to reset the whole microcontroller,
    // so safety is not such a huge concern.
    let aircr = 0xE000ED0C as *mut u32;
    unsafe { *aircr = (0x5FA<<16) | (1<<2) };
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
