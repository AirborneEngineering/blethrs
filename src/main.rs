#![no_std]

extern crate cortex_m;
extern crate cortex_m_semihosting;

#[macro_use]
extern crate stm32f4;

extern crate smoltcp;

use core::fmt::Write;

use cortex_m::asm;
use cortex_m_semihosting::hio;

use stm32f4::stm32f407;

mod ethernet;
mod network;

/// Set up PLL to 168MHz from 16MHz HSI
fn rcc_init(peripherals: &mut stm32f407::Peripherals) {
    let rcc = &peripherals.RCC;
    let flash = &peripherals.FLASH;
    let syscfg = &peripherals.SYSCFG;

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

    // Set system clock to HSI
    rcc.cfgr.modify(|_, w| w.sw().hsi());
    while !rcc.cfgr.read().sws().is_hsi() {}

    // Clear registers to reset value
    rcc.cr.write(|w| w.hsion().set_bit());
    rcc.cfgr.write(|w| unsafe { w.bits(0) });

    // Configure PLL: 16MHz /8 *168 /2, source HSI
    rcc.pllcfgr.write(|w| unsafe {
        w.pllq().bits(4)
         .pllsrc().hsi()
         .pllp().div2()
         .plln().bits(168)
         .pllm().bits(8)
    });
    // Activate PLL
    rcc.cr.modify(|_, w| w.pllon().set_bit());

    // Set other clock domains: PPRE2 to /2, PPRE1 to /4, HPRE to /1
    rcc.cfgr.modify(|_, w|
        w.ppre2().div2()
         .ppre1().div4()
         .hpre().div1());

    // Flash setup: I$ and D$ enabled, prefetch enabled, 5 wait states (OK for 3.3V at 168MHz)
    flash.acr.write(|w|
        w.icen().set_bit()
         .dcen().set_bit()
         .prften().set_bit()
         .latency().bits(5)
    );

    // Swap system clock to PLL
    rcc.cfgr.modify(|_, w| w.sw().pll());
    while !rcc.cfgr.read().sws().is_pll() {}

    // Set SYSCFG early to RMII mode
    rcc.apb2enr.modify(|_, w| w.syscfgen().enabled());
    syscfg.pmc.modify(|_, w| w.mii_rmii_sel().set_bit());

    // Set up peripheral clocks
    rcc.ahb1enr.modify(|_, w|
        w.gpioaen().enabled()
         .gpioben().enabled()
         .gpiocen().enabled()
         .gpioeen().enabled()
         .ethmacrxen().enabled()
         .ethmactxen().enabled()
         .ethmacen().enabled()
    );
}

fn gpio_init(peripherals: &mut stm32f407::Peripherals) {
    let gpioa = &peripherals.GPIOA;
    let gpiob = &peripherals.GPIOB;
    let gpioc = &peripherals.GPIOC;
    let gpioe = &peripherals.GPIOE;

    // Status LED
    gpioe.moder.modify(|_, w| w.moder7().output());

    // Configure ethernet related GPIO:
    // GPIOA 1, 2, 7
    // GPIOB 11, 12, 13
    // GPIOC 1, 4, 5
    // All set to AF11 and very high speed.
    gpioa.moder.modify(|_, w|
        w.moder1().alternate()
         .moder2().alternate()
         .moder7().alternate());
    gpiob.moder.modify(|_, w|
         w.moder11().alternate()
          .moder12().alternate()
          .moder13().alternate());
    gpioc.moder.modify(|_, w|
        w.moder1().alternate()
         .moder4().alternate()
         .moder5().alternate());
    gpioa.ospeedr.modify(|_, w|
        w.ospeedr1().very_high_speed()
         .ospeedr2().very_high_speed()
         .ospeedr7().very_high_speed());
    gpiob.ospeedr.modify(|_, w|
        w.ospeedr11().very_high_speed()
         .ospeedr12().very_high_speed()
         .ospeedr13().very_high_speed());
    gpioc.ospeedr.modify(|_, w|
        w.ospeedr1().very_high_speed()
         .ospeedr4().very_high_speed()
         .ospeedr5().very_high_speed());
    gpioa.afrl.modify(|_, w|
        w.afrl1().af11()
         .afrl2().af11()
         .afrl7().af11());
    gpiob.afrh.modify(|_, w|
        w.afrh11().af11()
         .afrh12().af11()
         .afrh13().af11());
    gpioc.afrl.modify(|_, w|
        w.afrl1().af11()
         .afrl4().af11()
         .afrl5().af11());
}

/// Set up the systick to provide a 1ms timebase
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

    rcc_init(&mut peripherals);
    gpio_init(&mut peripherals);
    systick_init(&mut core_peripherals.SYST);

    let mac_addr = smoltcp::wire::EthernetAddress::from_bytes(
        &[0x56, 0x54, 0x9f, 0x08, 0x87, 0x1d]);
    let mut ethdev = ethernet::EthernetDevice::new(
        peripherals.ETHERNET_MAC, peripherals.ETHERNET_DMA);
    ethdev.init(&mut peripherals.RCC, mac_addr);

    // Turn on STATUS LED
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
