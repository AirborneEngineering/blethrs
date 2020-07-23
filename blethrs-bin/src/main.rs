#![no_std]
#![no_main]

extern crate blethrs;
extern crate cortex_m;
extern crate cortex_m_rt;
extern crate cortex_m_semihosting;
extern crate panic_rtt_target;
extern crate rtt_target;
extern crate stm32f4xx_hal;
extern crate smoltcp;

use blethrs::{flash, Error};
use cortex_m_rt::{entry, exception};
use rtt_target::{rprintln, rtt_init_print};
use stm32f4xx_hal::stm32 as stm32f407;

mod ethernet;
mod network;

// Pull in build information (from `built` crate)
mod build_info {
    #![allow(dead_code)]
    include!(concat!(env!("OUT_DIR"), "/built.rs"));
}

// Default configuration.
mod default {
    const MAC_ADDR: [u8; 6] = [0x00, 0x00, 0xDE, 0xAD, 0xBE, 0xEF];
    const IP_ADDR: [u8; 4] = [169, 254, 141, 210];
    const IP_GATE: [u8; 4] = [IP_ADDR[0], IP_ADDR[1], IP_ADDR[2], 1];
    const IP_PREFIX: u8 = 24;

    pub fn config() -> blethrs::flash::UserConfig {
        blethrs::flash::UserConfig::new(MAC_ADDR, IP_ADDR, IP_GATE, IP_PREFIX)
    }
}

/// TCP port to listen on
const TCP_PORT: u16 = 7777;
/// PHY address
const ETH_PHY_ADDR: u8 = 0;

/// Only enter the user App if PD2 is high.
///
/// Check if PD2 is LOW for at least a full byte period of the UART, indicating someone has
/// connected 3V to the external connector.
fn app_entry_cond(peripherals: &mut stm32f407::Peripherals) -> bool {
    peripherals.RCC.ahb1enr.modify(|_, w| w.gpioden().enabled());
    peripherals.GPIOD.moder.modify(|_, w| w.moder2().input());
    let hsi_clk = 16_000_000;
    let sync_baud = 1_000_000;
    let bit_periods = 10;
    let delay = (hsi_clk / sync_baud) * bit_periods;
    let mut cond = true;
    for _ in 0..delay {
        cond &= peripherals.GPIOD.idr.read().idr2().bit_is_clear();
    }
    peripherals.RCC.ahb1enr.modify(|_, w| w.gpioden().disabled());
    !cond
}

/// Set up GPIOs for ethernet.
///
/// You should enable 9 GPIOs used by the ethernet controller. All GPIO clocks are already enabled.
/// This is also a sensible place to turn on an LED or similar to indicate bootloader mode.
fn configure_gpio(peripherals: &mut stm32f407::Peripherals) {
    let gpioa = &peripherals.GPIOA;
    let gpiob = &peripherals.GPIOB;
    let gpioc = &peripherals.GPIOC;
    let gpioe = &peripherals.GPIOE;

    // Status LED
    gpioe.moder.modify(|_, w| w.moder7().output());
    gpioe.odr.modify(|_, w| w.odr7().clear_bit());

    // Configure ethernet related GPIO:
    // GPIOA 1, 2, 7
    // GPIOC 1, 4, 5
    // GPIOG 11, 13, 14
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
    flash.acr.write(|w| unsafe {
        w.icen().set_bit()
         .dcen().set_bit()
         .prften().set_bit()
         .latency().bits(5)
    });

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
         .gpioden().enabled()
         .gpioeen().enabled()
         .gpiogen().enabled()
         .crcen().enabled()
         .ethmacrxen().enabled()
         .ethmactxen().enabled()
         .ethmacen().enabled()
    );
}

/// Set up the systick to provide a 1ms timebase
fn systick_init(syst: &mut stm32f407::SYST) {
    syst.set_reload((168_000_000 / 8) / 1000);
    syst.clear_current();
    syst.set_clock_source(cortex_m::peripheral::syst::SystClkSource::External);
    syst.enable_interrupt();
    syst.enable_counter();
}

#[entry]
fn main() -> ! {
    let mut peripherals = stm32f407::Peripherals::take().unwrap();
    let mut core_peripherals = stm32f407::CorePeripherals::take().unwrap();

    rtt_init_print!();

    // Jump to user code if it exists and hasn't asked us to run
    match flash::valid_user_code() {
        Some(address) => if !blethrs::bootload::should_enter(&mut peripherals.RCC) {
            if app_entry_cond(&mut peripherals) {
                blethrs::bootload::bootload(&mut core_peripherals.SCB, address);
            }
        },
        None => (),
    }

    rprintln!("\n|-=-=-=-=-=-=-=-=-= blethrs =-=-=-=-=-=-=-=-=-");
    rprintln!("| Version {} {}", build_info::PKG_VERSION, build_info::GIT_VERSION.unwrap());
    rprintln!("| Platform {}", build_info::TARGET);
    rprintln!("| Built on {}", build_info::BUILT_TIME_UTC);
    rprintln!("| {}", build_info::RUSTC_VERSION);
    rprintln!("|-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-\n");

    rprintln!(  " Initialising clocks...               ");
    rcc_init(&mut peripherals);
    rprintln!("OK");

    rprintln!(  " Initialising GPIOs...                ");
    configure_gpio(&mut peripherals);
    rprintln!("OK");

    rprintln!(  " Reading configuration...             ");
    let cfg = match flash::UserConfig::get(&mut peripherals.CRC) {
        Some(cfg) => { rprintln!("OK"); cfg },
        None => {
            rprintln!("No existing configuration. Using default.");
            default::config()
        },
    };

    //cfg.write_to_semihosting();
    let mac_addr = smoltcp::wire::EthernetAddress::from_bytes(&cfg.mac_address);

    rprintln!(  " Initialising Ethernet...             ");
    let mut ethdev = ethernet::EthernetDevice::new(
        peripherals.ETHERNET_MAC, peripherals.ETHERNET_DMA);
    ethdev.init(&mut peripherals.RCC, mac_addr.clone());
    rprintln!("OK");

    rprintln!(  " Waiting for link...                  ");
    ethdev.block_until_link();
    rprintln!("OK");

    rprintln!(  " Initialising network...              ");
    let ip_addr = smoltcp::wire::Ipv4Address::from_bytes(&cfg.ip_address);
    let ip_cidr = smoltcp::wire::Ipv4Cidr::new(ip_addr, cfg.ip_prefix);
    let cidr = smoltcp::wire::IpCidr::Ipv4(ip_cidr);
    network::init(ethdev, mac_addr.clone(), cidr);
    rprintln!("OK");

    // Move flash peripheral into flash module
    flash::init(peripherals.FLASH);

    // Turn on STATUS LED
    rprintln!(" Ready.\n");

    // Begin periodic tasks via systick
    systick_init(&mut core_peripherals.SYST);

    loop {
        core::sync::atomic::spin_loop_hint();
    }
}

static mut SYSTICK_TICKS: u32 = 0;
static mut SYSTICK_RESET_AT: Option<u32> = None;

#[exception]
fn SysTick() {
    let ticks = unsafe { core::ptr::read_volatile(&SYSTICK_TICKS) + 1 };
    unsafe { core::ptr::write_volatile(&mut SYSTICK_TICKS, ticks) };
    network::poll(ticks as i64);
    match unsafe { core::ptr::read_volatile(&SYSTICK_RESET_AT) } {
        Some(reset_time) => if ticks >= reset_time {
            rprintln!("Performing scheduled reset");
            blethrs::bootload::reset();
        },
        None => (),
    }
}

/// Reset after some ms delay.
pub fn schedule_reset(delay: u32) {
    cortex_m::interrupt::free(|_| unsafe {
        let ticks = core::ptr::read_volatile(&SYSTICK_TICKS) + delay;
        core::ptr::write_volatile(&mut SYSTICK_RESET_AT, Some(ticks));
    });
}

#[exception]
fn HardFault(ef: &cortex_m_rt::ExceptionFrame) -> ! {
    panic!("HardFault at {:#?}", ef);
}

#[exception]
fn DefaultHandler(irqn: i16) {
    panic!("Unhandled exception (IRQn = {})", irqn);
}
