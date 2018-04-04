#![no_std]

extern crate cortex_m;
extern crate cortex_m_semihosting;

extern crate stm32f4;

use core::fmt::Write;

use cortex_m_semihosting::hio;

use stm32f4::stm32f407;

fn main() {
    let mut stdout = hio::hstdout().unwrap();
    writeln!(stdout, "blethrs initialising").unwrap();

    let peripherals = stm32f407::Peripherals::take().unwrap();

    peripherals.RCC.ahb1enr.modify(|_, w| w.gpioeen().enabled());
    peripherals.GPIOE.moder.modify(|_, w| w.moder7().output());
    peripherals.GPIOE.odr.modify(|_, w| w.odr7().set_bit());

    writeln!(stdout, "entering main loop").unwrap();

    loop {}
}
