#![feature(core_intrinsics)]
#![feature(lang_items)]

#![no_std]

extern crate cortex_m;
extern crate cortex_m_semihosting;
extern crate panic_semihosting;

#[macro_use]
extern crate stm32f4;

extern crate smoltcp;
extern crate byteorder;

use stm32f4::stm32f407;

/// Try to print over semihosting if a debugger is available
#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => ({
        use core::fmt::Write;
        use cortex_m;
        use cortex_m_semihosting;
        if unsafe { (*cortex_m::peripheral::DCB::ptr()).dhcsr.read() & 1 == 1 } {
            match cortex_m_semihosting::hio::hstdout() {
                Ok(mut stdout) => {write!(stdout, $($arg)*).ok();},
                Err(_) => ()
            };
        }
    })
}

/// Try to print a line over semihosting if a debugger is available
#[macro_export]
macro_rules! println {
    ($fmt:expr) => (print!(concat!($fmt, "\n")));
    ($fmt:expr, $($arg:tt)*) => (print!(concat!($fmt, "\n"), $($arg)*));
}

mod ethernet;
mod network;
mod flash;
mod bootload;

// Pull in build information (from `built` crate)
pub mod build_info {
    include!(concat!(env!("OUT_DIR"), "/built.rs"));
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

fn gpio_init(peripherals: &mut stm32f407::Peripherals) {
    let gpioa = &peripherals.GPIOA;
    let gpioc = &peripherals.GPIOC;
    let gpiod = &peripherals.GPIOD;
    let gpiog = &peripherals.GPIOG;

    // Status LED
    gpiod.moder.modify(|_, w| w.moder3().output());

    // Configure ethernet related GPIO:
    // GPIOA 1, 2, 7
    // GPIOB 11, 12, 13
    // GPIOC 1, 4, 5
    // GPIOG 11, 13, 14
    // All set to AF11 and very high speed.
    gpioa.moder.modify(|_, w|
        w.moder1().alternate()
         .moder2().alternate()
         .moder7().alternate());
    gpiog.moder.modify(|_, w|
         w.moder11().alternate()
          .moder14().alternate()
          .moder13().alternate());
    gpioc.moder.modify(|_, w|
        w.moder1().alternate()
         .moder4().alternate()
         .moder5().alternate());
    gpioa.ospeedr.modify(|_, w|
        w.ospeedr1().very_high_speed()
         .ospeedr2().very_high_speed()
         .ospeedr7().very_high_speed());
    gpiog.ospeedr.modify(|_, w|
        w.ospeedr11().very_high_speed()
         .ospeedr14().very_high_speed()
         .ospeedr13().very_high_speed());
    gpioc.ospeedr.modify(|_, w|
        w.ospeedr1().very_high_speed()
         .ospeedr4().very_high_speed()
         .ospeedr5().very_high_speed());
    gpioa.afrl.modify(|_, w|
        w.afrl1().af11()
         .afrl2().af11()
         .afrl7().af11());
    gpiog.afrh.modify(|_, w|
        w.afrh11().af11()
         .afrh14().af11()
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
    let mut peripherals = stm32f407::Peripherals::take().unwrap();
    let mut core_peripherals = stm32f407::CorePeripherals::take().unwrap();

    // Jump to user code if it exists and hasn't asked us to run
    if bootload::should_bootload(&mut peripherals.RCC) {
        match flash::valid_user_code() {
            Some(address) => bootload::bootload(&mut core_peripherals.SCB, address),
            None => (),
        }
    }

    println!("");
    println!("|-=-=-=-=-=-=-=-=-= blethrs =-=-=-=-=-=-=-=-=-");
    println!("| Version {} {}", build_info::PKG_VERSION, build_info::GIT_VERSION.unwrap());
    println!("| Platform {}", build_info::TARGET);
    println!("| Built on {}", build_info::BUILT_TIME_UTC);
    println!("| {}", build_info::RUSTC_VERSION);
    println!("|-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-\n");

    print!(  " Initialising clocks...               ");
    rcc_init(&mut peripherals);
    println!("OK");

    print!(  " Initialising GPIOs...                ");
    gpio_init(&mut peripherals);
    println!("OK");

    print!(  " Reading configuration...             ");
    let cfg = flash::UserConfig::get(&mut peripherals.CRC);
    let mac_addr = smoltcp::wire::EthernetAddress::from_bytes(&cfg.mac_address);
    let ip_addr = smoltcp::wire::Ipv4Address::from_bytes(&cfg.ip_address);
    let gateway = smoltcp::wire::Ipv4Address::from_bytes(&cfg.ip_gateway);
    let ip_cidr = smoltcp::wire::Ipv4Cidr::new(ip_addr, cfg.ip_prefix);
    println!("OK");
    println!("   MAC Address: {}", mac_addr);
    println!("   IP Address:  {}", ip_cidr);
    println!("   Gateway:     {}", gateway);

    print!(  " Initialising Ethernet...             ");
    let mut ethdev = ethernet::EthernetDevice::new(
        peripherals.ETHERNET_MAC, peripherals.ETHERNET_DMA);
    ethdev.init(&mut peripherals.RCC, mac_addr.clone());
    println!("OK");

    print!(  " Waiting for link...                  ");
    ethdev.block_until_link();
    println!("OK");

    print!(  " Initialising network...              ");
    let cidr = smoltcp::wire::IpCidr::Ipv4(ip_cidr);
    unsafe { network::init(ethdev, mac_addr.clone(), cidr) };
    println!("OK");

    // Move flash peripheral into flash module
    flash::init(peripherals.FLASH);

    // Turn on STATUS LED
    peripherals.GPIOD.odr.modify(|_, w| w.odr3().clear_bit());
    println!(" Ready.\n");

    // Begin periodic tasks via systick
    systick_init(&mut core_peripherals.SYST);

    // When main returns, cortex-m-rt goes into an infinite wfi() loop.
}

static mut SYSTICK_TICKS: u32 = 0;
exception!(SYS_TICK, tick);
fn tick() {
    unsafe {
        let ticks = core::ptr::read_volatile(&SYSTICK_TICKS) + 1;
        core::ptr::write_volatile(&mut SYSTICK_TICKS, ticks);
        network::poll(ticks as i64);
    }
}
