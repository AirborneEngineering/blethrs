use core;
use cortex_m;
use stm32f407;

use smoltcp::{self, phy::{self, DeviceCapabilities}, time::Instant, wire::EthernetAddress};

const ETH_BUF_SIZE: usize = 1536;
const ETH_NUM_TD: usize = 4;
const ETH_NUM_RD: usize = 4;

use ::config::ETH_PHY_ADDR;

/// Transmit Descriptor representation
///
/// * tdes0: ownership bit and transmit settings
/// * tdes1: transmit buffer lengths
/// * tdes2: transmit buffer address
/// * tdes3: not used
///
/// Note that Copy and Clone are derived to support initialising an array of TDes,
/// but you may not move a TDes after its address has been given to the ETH_DMA engine.
#[derive(Copy,Clone)]
#[repr(C,packed)]
struct TDes {
    tdes0: u32,
    tdes1: u32,
    tdes2: u32,
    tdes3: u32,
}

impl TDes {
    /// Initialises this TDes to point at the given buffer.
    pub fn init(&mut self, tdbuf: &[u32]) {
        // Set FS and LS on each descriptor: each will hold a single full segment.
        self.tdes0 = (1<<29) | (1<<28);
        // Store pointer to associated buffer.
        self.tdes2 = tdbuf.as_ptr() as u32;
        // No second buffer.
        self.tdes3 = 0;
    }

    /// Mark this TDes as end-of-ring.
    pub fn set_end_of_ring(&mut self) {
        self.tdes0 |= 1<<21;
    }

    /// Return true if the RDes is not currently owned by the DMA
    pub fn available(&self) -> bool {
        self.tdes0 & (1<<31) == 0
    }

    /// Release this RDes back to DMA engine for transmission
    pub unsafe fn release(&mut self) {
        self.tdes0 |= 1<<31;
    }

    /// Set the length of data in the buffer pointed to by this TDes
    pub unsafe fn set_length(&mut self, length: usize) {
        self.tdes1 = (length as u32) & 0x1FFF;
    }

    /// Access the buffer pointed to by this descriptor
    pub unsafe fn buf_as_slice_mut(&self) -> &mut [u8] {
        core::slice::from_raw_parts_mut(self.tdes2 as *mut _, self.tdes1 as usize & 0x1FFF)
    }
}

/// Store a ring of TDes and associated buffers
struct TDesRing {
    td: [TDes; ETH_NUM_TD],
    tbuf: [[u32; ETH_BUF_SIZE/4]; ETH_NUM_TD],
    tdidx: usize,
}

static mut TDESRING: TDesRing = TDesRing {
    td: [TDes { tdes0: 0, tdes1: 0, tdes2: 0, tdes3: 0 }; ETH_NUM_TD],
    tbuf: [[0; ETH_BUF_SIZE/4]; ETH_NUM_TD],
    tdidx: 0,
};

impl TDesRing {
    /// Initialise this TDesRing
    ///
    /// The current memory address of the buffers inside this TDesRing will be stored in the
    /// descriptors, so ensure the TDesRing is not moved after initialisation.
    pub fn init(&mut self) {
        for (td, tdbuf) in self.td.iter_mut().zip(self.tbuf.iter()) {
            td.init(&tdbuf[..]);
        }
        self.td.last_mut().unwrap().set_end_of_ring();
    }

    /// Return the address of the start of the TDes ring
    pub fn ptr(&self) -> *const TDes {
        self.td.as_ptr()
    }

    /// Return true if a TDes is available for use
    pub fn available(&self) -> bool {
        self.td[self.tdidx].available()
    }

    /// Return the next available TDes if any are available, otherwise None
    pub fn next(&mut self) -> Option<&mut TDes> {
        if self.available() {
            let rv = Some(&mut self.td[self.tdidx]);
            self.tdidx = (self.tdidx + 1) % ETH_NUM_TD;
            rv
        } else {
            None
        }
    }
}

/// Receive Descriptor representation
///
/// * rdes0: ownership bit and received packet metadata
/// * rdes1: receive buffer lengths and settings
/// * rdes2: receive buffer address
/// * rdes3: not used
///
/// Note that Copy and Clone are derived to support initialising an array of TDes,
/// but you may not move a TDes after its address has been given to the ETH_DMA engine.
#[derive(Copy,Clone)]
#[repr(C,packed)]
struct RDes {
    rdes0: u32,
    rdes1: u32,
    rdes2: u32,
    rdes3: u32,
}

impl RDes {
    /// Initialises this RDes to point at the given buffer.
    pub fn init(&mut self, rdbuf: &[u32]) {
        // Mark each RDes as owned by the DMA engine.
        self.rdes0 = 1<<31;
        // Store length of and pointer to associated buffer.
        self.rdes1 = rdbuf.len() as u32 * 4;
        self.rdes2 = rdbuf.as_ptr() as u32;
        // No second buffer.
        self.rdes3 = 0;
    }

    /// Mark this RDes as end-of-ring.
    pub fn set_end_of_ring(&mut self) {
        self.rdes1 |= 1<<15;
    }

    /// Return true if the RDes is not currently owned by the DMA
    pub fn available(&self) -> bool {
        self.rdes0 & (1<<31) == 0
    }

    /// Release this RDes back to the DMA engine
    pub unsafe fn release(&mut self) {
        self.rdes0 |= 1<<31;
    }

    /// Access the buffer pointed to by this descriptor
    pub unsafe fn buf_as_slice(&self) -> &[u8] {
        core::slice::from_raw_parts(self.rdes2 as *const _, (self.rdes0 >> 16) as usize & 0x3FFF)
    }
}

/// Store a ring of RDes and associated buffers
struct RDesRing {
    rd: [RDes; ETH_NUM_RD],
    rbuf: [[u32; ETH_BUF_SIZE/4]; ETH_NUM_RD],
    rdidx: usize,
}

static mut RDESRING: RDesRing = RDesRing {
    rd: [RDes { rdes0: 0, rdes1: 0, rdes2: 0, rdes3: 0 }; ETH_NUM_RD],
    rbuf: [[0; ETH_BUF_SIZE/4]; ETH_NUM_RD],
    rdidx: 0,
};

impl RDesRing {
    /// Initialise this RDesRing
    ///
    /// The current memory address of the buffers inside this TDesRing will be stored in the
    /// descriptors, so ensure the TDesRing is not moved after initialisation.
    pub fn init(&mut self) {
        for (rd, rdbuf) in self.rd.iter_mut().zip(self.rbuf.iter()) {
            rd.init(&rdbuf[..]);
        }
        self.rd.last_mut().unwrap().set_end_of_ring();
    }

    /// Return the address of the start of the RDes ring
    pub fn ptr(&self) -> *const RDes {
        self.rd.as_ptr()
    }

    /// Return true if a RDes is available for use
    pub fn available(&self) -> bool {
        self.rd[self.rdidx].available()
    }

    /// Return the next available RDes if any are available, otherwise None
    pub fn next(&mut self) -> Option<&mut RDes> {
        if self.available() {
            let rv = Some(&mut self.rd[self.rdidx]);
            self.rdidx = (self.rdidx + 1) % ETH_NUM_RD;
            rv
        } else {
            None
        }
    }
}

/// Ethernet device driver
pub struct EthernetDevice {
    rdring: &'static mut RDesRing,
    tdring: &'static mut TDesRing,
    eth_mac: stm32f407::ETHERNET_MAC,
    eth_dma: stm32f407::ETHERNET_DMA,
}

static mut BUFFERS_USED: bool = false;

impl EthernetDevice {
    /// Create a new uninitialised EthernetDevice.
    ///
    /// You must move in ETH_MAC, ETH_DMA, and they are then kept by the device.
    ///
    /// You may only call this function once; subsequent calls will panic.
    pub fn new(eth_mac: stm32f407::ETHERNET_MAC, eth_dma: stm32f407::ETHERNET_DMA)
    -> EthernetDevice {
        cortex_m::interrupt::free(|_| unsafe {
            if BUFFERS_USED {
                panic!("EthernetDevice already created");
            }
            BUFFERS_USED = true;
            EthernetDevice { rdring: &mut RDESRING, tdring: &mut TDESRING, eth_mac, eth_dma }
        })
    }

    /// Initialise the ethernet driver.
    ///
    /// Sets up the descriptor structures, sets up the peripheral clocks and GPIO configuration,
    /// and configures the ETH MAC and DMA peripherals.
    ///
    /// Brings up the PHY and then blocks waiting for a network link.
    pub fn init(&mut self, rcc: &mut stm32f407::RCC, addr: EthernetAddress) {
        self.tdring.init();
        self.rdring.init();

        self.init_peripherals(rcc, addr);

        self.phy_reset();
        self.phy_init();
    }

    pub fn link_established(&mut self) -> bool {
        return self.phy_poll_link()
    }

    pub fn block_until_link(&mut self) {
        while !self.link_established() {}
    }

    /// Resume suspended TX DMA operation
    pub fn resume_tx_dma(&mut self) {
        if self.eth_dma.dmasr.read().tps().is_suspended() {
            self.eth_dma.dmatpdr.write(|w| w.tpd().poll());
        }
    }

    /// Resume suspended RX DMA operation
    pub fn resume_rx_dma(&mut self) {
        if self.eth_dma.dmasr.read().rps().is_suspended() {
            self.eth_dma.dmarpdr.write(|w| w.rpd().poll());
        }
    }

    /// Sets up the device peripherals.
    fn init_peripherals(&mut self, rcc: &mut stm32f407::RCC, mac: EthernetAddress) {
        // Reset ETH_MAC and ETH_DMA
        rcc.ahb1rstr.modify(|_, w| w.ethmacrst().reset());
        rcc.ahb1rstr.modify(|_, w| w.ethmacrst().clear_bit());
        self.eth_dma.dmabmr.modify(|_, w| w.sr().reset());
        while self.eth_dma.dmabmr.read().sr().is_reset() {}

        // Set MAC address
        let mac = mac.as_bytes();
        self.eth_mac.maca0lr.write(|w| w.maca0l().bits(
            (mac[0] as u32) << 0 | (mac[1] as u32) << 8 |
            (mac[2] as u32) <<16 | (mac[3] as u32) <<24));
        self.eth_mac.maca0hr.write(|w| w.maca0h().bits(
            (mac[4] as u16) << 0 | (mac[5] as u16) << 8));

        // Enable RX and TX. We'll set link speed and duplex at link-up.
        self.eth_mac.maccr.write(|w|
            w.re().enabled()
             .te().enabled()
             .cstf().enabled()
        );

        // Tell the ETH DMA the start of each ring
        self.eth_dma.dmatdlar.write(|w| w.stl().bits(self.tdring.ptr() as u32));
        self.eth_dma.dmardlar.write(|w| w.srl().bits(self.rdring.ptr() as u32));

        // Set DMA bus mode
        self.eth_dma.dmabmr.modify(|_, w|
            w.aab().aligned()
             .pbl().pbl1()
        );

        // Flush TX FIFO
        self.eth_dma.dmaomr.write(|w| w.ftf().flush());
        while self.eth_dma.dmaomr.read().ftf().is_flush() {}

        // Set DMA operation mode to store-and-forward and start DMA
        self.eth_dma.dmaomr.write(|w|
            w.rsf().store_forward()
             .tsf().store_forward()
             .st().started()
             .sr().started()
        );
    }

    /// Read a register over SMI.
    fn smi_read(&mut self, reg: u8) -> u16 {
        // Use PHY address 00000, set register address, set clock to HCLK/102, start read.
        self.eth_mac.macmiiar.write(|w|
            w.mb().busy()
             .pa().bits(ETH_PHY_ADDR)
             .cr().cr_150_168()
             .mr().bits(reg)
        );

        // Wait for read
        while self.eth_mac.macmiiar.read().mb().is_busy() {}

        // Return result
        self.eth_mac.macmiidr.read().md().bits()
    }

    /// Write a register over SMI.
    fn smi_write(&mut self, reg: u8, val: u16) {
        // Use PHY address 00000, set write data, set register address, set clock to HCLK/102,
        // start write operation.
        self.eth_mac.macmiidr.write(|w| w.md().bits(val));
        self.eth_mac.macmiiar.write(|w|
            w.mb().busy()
             .pa().bits(ETH_PHY_ADDR)
             .mw().write()
             .cr().cr_150_168()
             .mr().bits(reg)
        );

        while self.eth_mac.macmiiar.read().mb().is_busy() {}
    }

    /// Reset the connected PHY and wait for it to come out of reset.
    fn phy_reset(&mut self) {
        self.smi_write(0x00, 1<<15);
        while self.smi_read(0x00) & (1<<15) == (1<<15) {}
    }

    /// Command connected PHY to initialise.
    fn phy_init(&mut self) {
        self.smi_write(0x00, 1<<12);
    }

    /// Poll PHY to determine link status.
    fn phy_poll_link(&mut self) -> bool {
        let bsr = self.smi_read(0x01);
        let bcr = self.smi_read(0x00);
        let lpa = self.smi_read(0x05);

        // No link without autonegotiate
        if bcr & (1<<12) == 0 { return false; }
        // No link if link is down
        if bsr & (1<< 2) == 0 { return false; }
        // No link if remote fault
        if bsr & (1<< 4) != 0 { return false; }
        // No link if autonegotiate incomplete
        if bsr & (1<< 5) == 0 { return false; }
        // No link if other side can't do 100Mbps full duplex
        if lpa & (1<< 8) == 0 { return false; }

        // Got link. Configure MAC to 100Mbit/s and full duplex.
        self.eth_mac.maccr.modify(|_, w|
            w.fes().fes100()
             .dm().full_duplex()
        );

        true
    }
}

pub struct TxToken(*mut EthernetDevice);
pub struct RxToken(*mut EthernetDevice);

impl phy::TxToken for TxToken {
    fn consume<R, F>(self, _timestamp: Instant, len: usize, f: F) -> smoltcp::Result<R>
        where F: FnOnce(&mut [u8]) -> smoltcp::Result<R>
    {
        // There can only be a single EthernetDevice and therefore all TxTokens are wrappers
        // to a raw pointer to it. Unsafe required to dereference this pointer and call
        // the various TDes methods.
        assert!(len <= ETH_BUF_SIZE);
        unsafe {
            let tdes = (*self.0).tdring.next().unwrap();
            tdes.set_length(len);
            let result = f(tdes.buf_as_slice_mut());
            tdes.release();
            (*self.0).resume_tx_dma();
            result
        }
    }
}

impl phy::RxToken for RxToken {
    fn consume<R, F>(self, _timestamp: Instant, f: F) -> smoltcp::Result<R>
        where F: FnOnce(&[u8]) -> smoltcp::Result<R>
    {
        // There can only be a single EthernetDevice and therefore all RxTokens are wrappers
        // to a raw pointer to it. Unsafe required to dereference this pointer and call
        // the various RDes methods.
        unsafe {
            let rdes = (*self.0).rdring.next().unwrap();
            let result = f(rdes.buf_as_slice());
            rdes.release();
            (*self.0).resume_rx_dma();
            result
        }
    }
}

// Implement the smoltcp Device interface
impl<'a> phy::Device<'a> for EthernetDevice {
    type RxToken = RxToken;
    type TxToken = TxToken;

    fn capabilities(&self) -> DeviceCapabilities {
        let mut caps = DeviceCapabilities::default();
        caps.max_transmission_unit = 1500;
        caps.max_burst_size = Some(core::cmp::min(ETH_NUM_TD, ETH_NUM_RD));
        caps
    }

    fn receive(&mut self) -> Option<(RxToken, TxToken)> {
        if self.rdring.available() && self.tdring.available() {
            Some((RxToken(self), TxToken(self)))
        } else {
            None
        }
    }

    fn transmit(&mut self) -> Option<TxToken> {
        if self.tdring.available() {
            Some(TxToken(self))
        } else {
            None
        }
    }
}
