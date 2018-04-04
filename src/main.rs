#![no_std]

extern crate cortex_m;
extern crate cortex_m_semihosting;

#[macro_use]
extern crate stm32f4;

use core::fmt::Write;

use cortex_m::asm;
use cortex_m_semihosting::hio;

use stm32f4::stm32f407;

/// Set up PLL to 168MHz from 16MHz HSI
fn rcc_init(rcc: &mut stm32f407::RCC, flash: &mut stm32f407::FLASH) {
    // Reset all peripherals
    rcc.ahb1rstr.write(|w| unsafe { w.bits(0xFFFF_FFFF) });
    rcc.ahb1rstr.write(|w| unsafe { w.bits(0)});
    rcc.ahb2rstr.write(|w| unsafe { w.bits(0xFFFF_FFFF) });
    rcc.ahb2rstr.write(|w| unsafe { w.bits(0)});
    rcc.ahb3rstr.write(|w| unsafe { w.bits(0xFFFF_FFFF) });
    rcc.ahb3rstr.write(|w| unsafe { w.bits(0)});
    rcc.apb1rstr.write(|w| unsafe { w.bits(0xFFFF_FFFF) });
    rcc.apb1rstr.write(|w| unsafe { w.bits(0)});
    rcc.apb2rstr.write(|w| unsafe { w.bits(0xFFFF_FFFF) });
    rcc.apb2rstr.write(|w| unsafe { w.bits(0)});

    // Ensure HSI is on and stable
    rcc.cr.modify(|_, w| w.hsion().set_bit());
    while rcc.cr.read().hsion().bit_is_clear() {}

    // Set to HSI
    rcc.cfgr.modify(|_, w| w.sw().hsi());
    while !rcc.cfgr.read().sws().is_hsi() {}

    // Clear register to reset value
    rcc.cr.write(|w| w.hsion().set_bit());
    rcc.cfgr.write(|w| unsafe { w.bits(0) });

    // Activate PLL
    rcc.pllcfgr.write(|w| unsafe {
        w.pllq().bits(4)
         .pllsrc().hsi()
         .pllp().div2()
         .plln().bits(168)
         .pllm().bits(8)
    });
    rcc.cr.modify(|_, w| w.pllon().set_bit());

    // Other clock settings
    rcc.cfgr.modify(|_, w|
        w.ppre2().div2()
         .ppre1().div4()
         .hpre().div1());

    // Flash setup
    flash.acr.write(|w| unsafe {
        w.icen().set_bit()
         .dcen().set_bit()
         .prften().set_bit()
         .latency().bits(5)
    });

    // Swap to PLL
    rcc.cfgr.modify(|_, w| w.sw().pll());
    while !rcc.cfgr.read().sws().is_pll() {}
}

fn systick_init(syst: &mut stm32f407::SYST) {
    syst.set_reload((168_000_000 / 8) / 1000);
    syst.clear_current();
    syst.set_clock_source(cortex_m::peripheral::syst::SystClkSource::External);
    syst.enable_interrupt();
    syst.enable_counter();
}

fn main() {
    let mut stdout = hio::hstdout().unwrap();
    writeln!(stdout, "blethrs initialising").unwrap();

    let mut peripherals = stm32f407::Peripherals::take().unwrap();
    let mut core_peripherals = stm32f407::CorePeripherals::take().unwrap();

    rcc_init(&mut peripherals.RCC, &mut peripherals.FLASH);
    systick_init(&mut core_peripherals.SYST);

    peripherals.RCC.ahb1enr.modify(|_, w| w.gpioeen().enabled());
    peripherals.GPIOE.moder.modify(|_, w| w.moder7().output());
    peripherals.GPIOE.odr.modify(|_, w| w.odr7().set_bit());

    writeln!(stdout, "entering main loop").unwrap();

    loop {
        asm::wfi();
    }
}

static mut SYSTICK_TICKS: u32 = 0;
exception!(SYS_TICK, tick);
fn tick() {
    unsafe {
        let ticks = core::ptr::read_volatile(&SYSTICK_TICKS) + 1;
        core::ptr::write_volatile(&mut SYSTICK_TICKS, ticks);
    }
}
