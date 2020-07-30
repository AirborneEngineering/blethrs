#![no_main]
#![no_std]

use blethrs::flash::UserConfig;
use panic_rtt_target as _;
use rtic::app;
use rtic::cyccnt::U32Ext as CyccntU32Ext;
use rtt_target::{rtt_init_print, rprintln};
use smoltcp::{
    iface::{EthernetInterfaceBuilder, Neighbor, NeighborCache},
    socket::{SocketHandle, SocketSetItem, TcpSocket, TcpSocketBuffer},
    time::Instant,
    wire::{EthernetAddress, IpAddress, IpCidr},
};
use stm32_eth::{
    {EthPins, PhyAddress, RingEntry, RxDescriptor, TxDescriptor},
    hal::gpio::GpioExt,
    hal::rcc::RccExt,
    hal::time::U32Ext as TimeU32Ext,
};

// Pull in build information (from `built` crate).
mod build_info {
    #![allow(dead_code)]
    include!(concat!(env!("OUT_DIR"), "/built.rs"));
}

// Default values that may otherwise be configured via flash.
mod default {
    const MAC_ADDR: [u8; 6] = [0x00, 0x00, 0xDE, 0xAD, 0xBE, 0xEF];
    const IP_ADDR: [u8; 4] = [169, 254, 141, 210];
    const IP_GATE: [u8; 4] = [IP_ADDR[0], IP_ADDR[1], IP_ADDR[2], 1];
    const IP_PREFIX: u8 = 24;

    pub fn config() -> blethrs::flash::UserConfig {
        blethrs::flash::UserConfig::new(MAC_ADDR, IP_ADDR, IP_GATE, IP_PREFIX)
    }
}

type Eth = stm32_eth::Eth<'static, 'static>;
type EthernetInterface = smoltcp::iface::EthernetInterface<'static, 'static, 'static, &'static mut Eth>;
type SocketSet = smoltcp::socket::SocketSet<'static, 'static, 'static>;

const CYCLE_HZ: u32 = 168_000_000;
const ONE_SEC: u32 = CYCLE_HZ;
const ONE_MS: u32 = ONE_SEC / 1_000;
const PORT: u16 = 7777;
const MTU: usize = 1536;

#[app(device = blethrs::stm32, peripherals = true, monotonic = rtic::cyccnt::CYCCNT)]
const APP: () = {
    struct Resources {
        #[init(0)]
        now_ms: u32,
        #[init(None)]
        reset_ms: Option<u32>,
        eth_iface: EthernetInterface,
        sockets: SocketSet,
        server_handle: SocketHandle,
    }

    #[init(schedule = [every_ms])]
    fn init(mut cx: init::Context) -> init::LateResources {
        rtt_init_print!();

        let cause = match blethrs::flash::valid_user_code() {
            Some(address) if !blethrs::bootload::should_enter_bootloader(&mut cx.device.RCC) => {
                rprintln!("Loading user program!");
                blethrs::bootload::bootload(&mut cx.core.SCB, address);
                loop {
                    core::sync::atomic::spin_loop_hint();
                }
            },
            Some(_addr) => "User indicated",
            None => "Invalid user program",
        };

        rprintln!("Running in bootload mode. Cause: {}.", cause);

        rprintln!("Setup clocks");
        cx.core.DWT.enable_cycle_counter();
        let rcc = cx.device.RCC.constrain();
        let clocks = rcc.cfgr.sysclk(CYCLE_HZ.hz()).freeze();

        rprintln!("Reading user config");
        let cfg = match UserConfig::get(&mut cx.device.CRC) {
            Some(cfg) => cfg,
            None => {
                rprintln!("  No existing configuration. Using default.");
                default::config()
            },
        };

        rprintln!("Setup ethernet");
        let gpioa = cx.device.GPIOA.split();
        let gpiob = cx.device.GPIOB.split();
        let gpioc = cx.device.GPIOC.split();
        let eth_pins = EthPins {
            ref_clk: gpioa.pa1,
            md_io: gpioa.pa2,
            md_clk: gpioc.pc1,
            crs: gpioa.pa7,
            tx_en: gpiob.pb11,
            tx_d0: gpiob.pb12,
            tx_d1: gpiob.pb13,
            rx_d0: gpioc.pc4,
            rx_d1: gpioc.pc5,
        };
        let eth = {
            static mut RX_RING: Option<[RingEntry<RxDescriptor>; 8]> = None;
            static mut TX_RING: Option<[RingEntry<TxDescriptor>; 2]> = None;
            static mut ETH: Option<Eth> = None;
            unsafe {
                RX_RING = Some(Default::default());
                TX_RING = Some(Default::default());
                let eth = Eth::new(
                    cx.device.ETHERNET_MAC,
                    cx.device.ETHERNET_DMA,
                    &mut RX_RING.as_mut().unwrap()[..],
                    &mut TX_RING.as_mut().unwrap()[..],
                    PhyAddress::_0,
                    clocks,
                    eth_pins,
                ).unwrap();
                ETH = Some(eth);
                ETH.as_mut().unwrap()
            }
        };
        eth.enable_interrupt();

        rprintln!("Setup TCP/IP");
        let [a0, a1, a2, a3] = cfg.ip_address;
        let ip_addr = IpCidr::new(IpAddress::v4(a0, a1, a2, a3), cfg.ip_prefix);
        let (ip_addrs, neighbor_storage) = {
            static mut IP_ADDRS: Option<[IpCidr; 1]> = None;
            static mut NEIGHBOR_STORAGE: [Option<(IpAddress, Neighbor)>; 16] = [None; 16];
            unsafe {
                IP_ADDRS = Some([ip_addr]);
                (IP_ADDRS.as_mut().unwrap(), &mut NEIGHBOR_STORAGE)
            }
        };
        let neighbor_cache = NeighborCache::new(&mut neighbor_storage[..]);
        let ethernet_addr = EthernetAddress(cfg.mac_address);
        let eth_iface = EthernetInterfaceBuilder::new(eth)
            .ethernet_addr(ethernet_addr)
            .ip_addrs(&mut ip_addrs[..])
            .neighbor_cache(neighbor_cache)
            .finalize();
        let (server_socket, mut sockets) = {
            static mut RX_BUFFER: [u8; MTU] = [0; MTU];
            static mut TX_BUFFER: [u8; MTU] = [0; MTU];
            static mut SOCKETS_STORAGE: [Option<SocketSetItem>; 2] = [None, None];
            unsafe {
                let server_socket = TcpSocket::new(
                    TcpSocketBuffer::new(&mut RX_BUFFER[..]),
                    TcpSocketBuffer::new(&mut TX_BUFFER[..]),
                );
                let sockets = SocketSet::new(&mut SOCKETS_STORAGE[..]);
                (server_socket, sockets)
            }
        };
        let server_handle = sockets.add(server_socket);

        // Move flash peripheral into flash module
        blethrs::flash::init(cx.device.FLASH);

        // Schedule the `blink` and `every_ms` tasks.
        cx.schedule.every_ms(cx.start + ONE_MS.cycles()).unwrap();

        rprintln!("Run!");
        init::LateResources { eth_iface, sockets, server_handle }
    }

    #[task(resources = [now_ms, reset_ms], schedule = [every_ms])]
    fn every_ms(mut cx: every_ms::Context) {
        let r = &mut cx.resources;
        *r.now_ms = r.now_ms.wrapping_add(1);

        // Check for a reset countdown.
        if let Some(ref mut ms) = *r.reset_ms {
            *ms = ms.saturating_sub(1);
            if *ms == 0 {
                blethrs::bootload::reset();
            }
        }

        cx.schedule.every_ms(cx.scheduled + ONE_MS.cycles()).unwrap();
    }

    #[task(binds = ETH, resources = [eth_iface, now_ms, sockets, server_handle, reset_ms])]
    fn eth(mut cx: eth::Context) {
        let r = &mut cx.resources;
        // Clear interrupt flags.
        r.eth_iface.device_mut().interrupt_handler();
        poll_eth_iface(r.eth_iface, r.sockets, *r.server_handle, *r.now_ms, r.reset_ms);
    }

    #[idle]
    fn idle(_: idle::Context) -> ! {
        loop {
            core::sync::atomic::spin_loop_hint();
        }
    }

    extern "C" {
        fn EXTI0();
    }
};

fn build_info() -> blethrs::cmd::BuildInfo<'static> {
    blethrs::cmd::BuildInfo {
        pkg_version: build_info::PKG_VERSION,
        git_version: build_info::GIT_VERSION.expect("no git version found"),
        built_time_utc: build_info::BUILT_TIME_UTC,
        rustc_version: build_info::RUSTC_VERSION,
    }
}

fn poll_eth_iface(
    iface: &mut EthernetInterface,
    sockets: &mut SocketSet,
    server_handle: SocketHandle,
    now_ms: u32,
    reset_ms: &mut Option<u32>,
) {
    {
        let mut socket = sockets.get::<TcpSocket>(server_handle);
        handle_tcp(&mut socket, reset_ms);
    }

    let now = Instant::from_millis(now_ms as i64);
    if let Err(e) = iface.poll(sockets, now) {
        rprintln!("An error occurred when polling: {}", e);
    }
}

fn handle_tcp(socket: &mut TcpSocket, reset_ms: &mut Option<u32>) {
    if !socket.is_open() {
        if let Err(e) = socket.listen(PORT) {
            panic!("failed to listen on port {} of TCP socket: {}", PORT, e);
        }
    }

    if !socket.may_recv() && socket.may_send() {
        socket.close();
    }

    if socket.can_recv() {
        let mut cmd = [0u8; 4];
        socket.recv_slice(&mut cmd[..]).ok();
        let cmd = u32::from_le_bytes(cmd);
        let build_info = build_info();
        match blethrs::cmd::handle_and_respond(cmd, &build_info, socket) {
            Ok(reboot) if reboot => {
                rprintln!("Resetting...");
                *reset_ms = Some(50);
            },
            Err(_e) => rprintln!("Received unknown command: {}", cmd),
            _ => (),
        }

        socket.close();
    }
}
