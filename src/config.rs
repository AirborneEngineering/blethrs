//! Chip and board specific configuration settings go here.
use stm32f407;
use ::bootload;

/// TCP port to listen on
pub const TCP_PORT: u16 = 7777;

/// PHY address
pub const ETH_PHY_ADDR: u8 = 0;

/// Start address of each sector in flash
pub const FLASH_SECTOR_ADDRESSES: [u32; 12] =
    [0x0800_0000, 0x0800_4000, 0x0800_8000, 0x0800_C000,
     0x0801_0000, 0x0802_0000, 0x0804_0000, 0x0806_0000,
     0x0808_0000, 0x080A_0000, 0x080C_0000, 0x080E_0000];
/// Final valid address in flash
pub const FLASH_END: u32 = 0x080F_FFFF;
/// Address of configuration sector. Must be one of the start addresses in FLASH_SECTOR_ADDRESSES.
pub const FLASH_CONFIG: u32 = FLASH_SECTOR_ADDRESSES[3];
/// Address of user firmware sector. Must be one of the start addresses in FLASH_SECTOR_ADDRESSES.
pub const FLASH_USER: u32   = FLASH_SECTOR_ADDRESSES[4];

/// Magic value used in this module to check if bootloader should start.
pub const BOOTLOAD_FLAG_VALUE: u32 = 0xB00110AD;
/// Address of magic value used in this module to check if bootloader should start.
pub const BOOTLOAD_FLAG_ADDRESS: u32 = 0x2000_0000;

/// This function should return true if the bootloader should enter bootload mode,
/// or false to immediately chainload the user firmware.
///
/// By default we check if there was a software reset and a magic value is set in RAM,
/// but you could also check GPIOs etc here.
///
/// Ensure any state change to the peripherals is reset before returning from this function.
pub fn should_enter_bootloader(peripherals: &mut stm32f407::Peripherals) -> bool {
    // Our plan is:
    // * If the reset was a software reset, and the magic flag is in the magic location,
    //   then the user firmware requested bootload, so enter bootload.
    bootload::was_software_reset(&mut peripherals.RCC) && bootload::flag_set()
}

/// Set up GPIOs for ethernet.
///
/// You should enable 9 GPIOs used by the ethernet controller. All GPIO clocks are already enabled.
/// This is also a sensible place to turn on an LED or similar to indicate bootloader mode.
pub fn configure_gpio(peripherals: &mut stm32f407::Peripherals) {
    let gpioa = &peripherals.GPIOA;
    let gpiob = &peripherals.GPIOB;
    let gpioc = &peripherals.GPIOC;
    let gpiog = &peripherals.GPIOG;

    // Status LED
    gpiob.moder.modify(|_, w| w.moder4().output());
    gpiob.odr.modify(|_, w| w.odr4().set_bit());

    // PHY RESET
    gpioa.moder.modify(|_, w| w.moder0().output());
    gpioa.odr.modify(|_, w| w.odr0().set_bit());

    // Configure ethernet related GPIO.
    // All set to AF11 and very high speed.
    gpioa.moder.modify(|_, w|
        w.moder1().alternate()
         .moder2().alternate()
         .moder7().alternate());
    gpioc.moder.modify(|_, w|
        w.moder1().alternate()
         .moder4().alternate()
         .moder5().alternate());
    gpiog.moder.modify(|_, w|
        w.moder11().alternate()
         .moder13().alternate()
         .moder14().alternate());
    gpioa.ospeedr.modify(|_, w|
        w.ospeedr1().very_high_speed()
         .ospeedr2().very_high_speed()
         .ospeedr7().very_high_speed());
    gpioc.ospeedr.modify(|_, w|
        w.ospeedr1().very_high_speed()
         .ospeedr4().very_high_speed()
         .ospeedr5().very_high_speed());
    gpiog.ospeedr.modify(|_, w|
        w.ospeedr11().very_high_speed()
         .ospeedr13().very_high_speed()
         .ospeedr14().very_high_speed());
    gpioa.afrl.modify(|_, w|
        w.afrl1().af11()
         .afrl2().af11()
         .afrl7().af11());
    gpioc.afrl.modify(|_, w|
        w.afrl1().af11()
         .afrl4().af11()
         .afrl5().af11());
    gpiog.afrh.modify(|_, w|
        w.afrh11().af11()
         .afrh13().af11()
         .afrh14().af11());
}
