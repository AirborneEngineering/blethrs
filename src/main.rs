#![feature(used)]
#![no_std]

extern crate vcell;
extern crate cortex_m;
extern crate cortex_m_rt;
extern crate cortex_m_semihosting;

use core::fmt::Write;
use core::ops::Deref;

use cortex_m::asm;
use cortex_m_semihosting::hio;

mod gpio {
    use vcell::VolatileCell;
    #[allow(non_snake_case)]
    pub struct RegisterBlock {
        pub MODER: ModeR,
        pub OTYPER: VolatileCell<u32>,
        pub OSPEEDR: VolatileCell<u32>,
        pub PUPDR: VolatileCell<u32>,
        pub IDR: VolatileCell<u32>,
        pub ODR: VolatileCell<u32>,
        pub BSRR: VolatileCell<u32>,
        pub LCKR: VolatileCell<u32>,
        pub AFRL: VolatileCell<u32>,
        pub AFRH: VolatileCell<u32>,
    }
    pub struct ModeR {
        pub register: VolatileCell<u32>,
    }
    pub enum Mode {
        Input = 0b00,
        Output = 0b01,
        Alternate = 0b10,
        Analog = 0b11,
    }
    impl ModeR {
        pub fn set(&self, pin: u32, mode: Mode) {
            let mut curval = self.register.get();
            curval |= (mode as u32) << (pin * 2);
            self.register.set(curval);
        }
    }
}

struct GPIOE {}

impl Deref for GPIOE {
    type Target = gpio::RegisterBlock;
    fn deref(&self) -> &gpio::RegisterBlock {
        unsafe { &*(0x40021000 as *const _)}
    }
}

const GPIOE: GPIOE = GPIOE {};

mod rcc {
    use vcell::VolatileCell;
    #[allow(non_snake_case)]
    pub struct RegisterBlock {
        pub CR: VolatileCell<u32>,
        pub PLLCFGR: VolatileCell<u32>,
        pub CFGR: VolatileCell<u32>,
        pub CIR: VolatileCell<u32>,
        pub AHB1RSTR: VolatileCell<u32>,
        pub AHB2RSTR: VolatileCell<u32>,
        pub AHB3RSTR: VolatileCell<u32>,
        _0: VolatileCell<u32>,
        pub APB1RSTR: VolatileCell<u32>,
        pub APB2RST: VolatileCell<u32>,
        _1: VolatileCell<u32>,
        _2: VolatileCell<u32>,
        pub AHB1ENR: VolatileCell<u32>,
        pub AHB2ENR: VolatileCell<u32>,
        pub AHB3ENR: VolatileCell<u32>,
        _3: VolatileCell<u32>,
        pub APB1ENR: VolatileCell<u32>,
        pub APB2ENR: VolatileCell<u32>,
        _4: VolatileCell<u32>,
        _5: VolatileCell<u32>,
        pub AHB1LPENR: VolatileCell<u32>,
        pub AHB2LPENR: VolatileCell<u32>,
        pub AHB3LPENR: VolatileCell<u32>,
        _6: VolatileCell<u32>,
        pub APB1LPENR: VolatileCell<u32>,
        pub APB2LPENR: VolatileCell<u32>,
        _7: VolatileCell<u32>,
        _8: VolatileCell<u32>,
        pub BDCR: VolatileCell<u32>,
        pub CSR: VolatileCell<u32>,
        _9: VolatileCell<u32>,
        _10: VolatileCell<u32>,
        pub SSCGR: VolatileCell<u32>,
        pub PLLI2SCFGR: VolatileCell<u32>,
        pub PLLSAICFGR: VolatileCell<u32>,
        pub DCKCFGR: VolatileCell<u32>,
    }
}

struct RCC {}

impl Deref for RCC {
    type Target = rcc::RegisterBlock;
    fn deref(&self) -> &rcc::RegisterBlock {
        unsafe { &*(0x40023800 as *const _) }
    }
}

const RCC: RCC = RCC {};

fn main() {
    let mut stdout = hio::hstdout().unwrap();
    writeln!(stdout, "blethrs initialising").unwrap();

    RCC.AHB1ENR.set((1<<4) | (1<<20));

    GPIOE.MODER.set(7, gpio::Mode::Output);
    GPIOE.ODR.set(1<<7);

    writeln!(stdout, "entering main loop").unwrap();

    loop {}
}

#[link_section = ".vector_table.interrupts"]
#[used]
static INTERRUPTS: [extern "C" fn(); 240] = [default_handler; 240];

extern "C" fn default_handler() {
    asm::bkpt();
}
