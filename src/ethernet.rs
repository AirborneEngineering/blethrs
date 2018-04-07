use core;
use smoltcp;
use stm32f407;

use self::smoltcp::phy::{Device, DeviceLimits};
use self::smoltcp::wire::EthernetAddress;

/// Transmit Descriptor representation
///
/// * tdes0: ownership bit and transmit settings
/// * tdes1: transmit buffer lengths
/// * tdes2: transmit buffer address
/// * tdes3: not used
#[repr(C,packed)]
#[derive(Copy,Clone)]
struct TDes {
    tdes0: u32,
    tdes1: u32,
    tdes2: u32,
    tdes3: u32,
}

/// Receive Descriptor representation
///
/// * rdes0: ownership bit and received packet metadata
/// * rdes1: receive buffer lengths and settings
/// * rdes2: receive buffer address
/// * rdes3: not used
#[repr(C,packed)]
#[derive(Copy,Clone)]
struct RDes {
    rdes0: u32,
    rdes1: u32,
    rdes2: u32,
    rdes3: u32,
}

const ETH_BUF_SIZE: usize = 1536;
const ETH_NUM_TD: usize = 8;
const ETH_NUM_RD: usize = 2;

/// Ethernet device driver
pub struct EthernetDevice {
    tbuf: [[u32; ETH_BUF_SIZE/4]; ETH_NUM_TD],
    rbuf: [[u32; ETH_BUF_SIZE/4]; ETH_NUM_RD],
    td: [TDes; ETH_NUM_TD],
    rd: [RDes; ETH_NUM_RD],
    tdidx: usize,
    rdidx: usize,

    eth_mac: stm32f407::ETHERNET_MAC,
    eth_dma: stm32f407::ETHERNET_DMA,
}

impl EthernetDevice {
    /// Create a new uninitialised EthernetDevice.
    ///
    /// You must move in ETH_MAC, ETH_DMA, and they are then kept by the device.
    pub fn new(eth_mac: stm32f407::ETHERNET_MAC, eth_dma: stm32f407::ETHERNET_DMA)
    -> EthernetDevice {
        EthernetDevice {
            tbuf: [[0; ETH_BUF_SIZE/4]; ETH_NUM_TD],
            rbuf: [[0; ETH_BUF_SIZE/4]; ETH_NUM_RD],
            td: [TDes {tdes0: 0, tdes1: 0, tdes2: 0, tdes3: 0}; ETH_NUM_TD],
            rd: [RDes {rdes0: 0, rdes1: 0, rdes2: 0, rdes3: 0}; ETH_NUM_RD],
            tdidx: 0, rdidx: 0,
            eth_mac, eth_dma,
        }
    }

    /// Initialise the ethernet driver.
    ///
    /// Sets up the descriptor structures, sets up the peripheral clocks and GPIO configuration,
    /// and configures the ETH MAC and DMA peripherals.
    ///
    /// Brings up the PHY and then blocks waiting for a network link.
    pub fn init(&mut self, rcc: &mut stm32f407::RCC, addr: EthernetAddress) {
        self.init_descriptors();
        self.init_peripherals(rcc, addr);

        // Set up the PHY
        self.phy_reset();
        self.phy_init();

        // Wait for network link
        while !self.phy_poll_link() {}
    }

    /// Resume suspended TX DMA operation
    pub fn resume_tx_dma(&mut self) {
        if self.eth_dma.dmasr.read().tps().is_suspended() {
            self.eth_dma.dmatpdr.write(|w| w.tpd().bits(0));
        }
    }

    /// Resume suspended RX DMA operation
    pub fn resume_rx_dma(&mut self) {
        if self.eth_dma.dmasr.read().rps().is_suspended() {
            self.eth_dma.dmarpdr.write(|w| w.rpd().bits(0));
        }
    }

    /// Set up descriptor structure.
    fn init_descriptors(&mut self) {
        // Set up each TDes in ring mode with associated buffer
        for (td, tdbuf) in self.td.iter_mut().zip(self.tbuf.iter()) {
            td.tdes0 = (1<<29) | (1<<28);
            td.tdes2 = tdbuf as *const _ as u32;
            td.tdes3 = 0;
        }

        // Set up each RDes in ring mode with associated buffer
        for (rd, rdbuf) in self.rd.iter_mut().zip(self.rbuf.iter()) {
            rd.rdes0 = 1<<31;
            rd.rdes1 = rdbuf.len() as u32 * 4;
            rd.rdes2 = rdbuf as *const _ as u32;
            rd.rdes3 = 0;
        }

        // Mark final TDes and RDes as end-of-ring
        self.td.last_mut().unwrap().tdes0 |= 1<<21;
        self.rd.last_mut().unwrap().rdes1 |= 1<<15;
    }

    /// Sets up the device peripherals.
    fn init_peripherals(&mut self, rcc: &mut stm32f407::RCC, mac: EthernetAddress) {
        // Reset ETH_MAC and ETH_DMA
        rcc.ahb1rstr.modify(|_, w| w.ethmacrst().reset());
        self.eth_dma.dmabmr.modify(|_, w| w.sr().reset());
        while self.eth_dma.dmabmr.read().sr().is_reset() {}
        rcc.ahb1rstr.modify(|_, w| w.ethmacrst().clear_bit());

        // Set MAC address
        let mac = mac.as_bytes();
        self.eth_mac.maca0lr.write(|w| w.maca0l().bits(
            (mac[0] as u32) << 0 |
            (mac[1] as u32) << 8 |
            (mac[2] as u32) <<16 |
            (mac[3] as u32) <<24));
        self.eth_mac.maca0hr.write(|w| w.maca0h().bits(
            (mac[4] as u16) << 0 |
            (mac[5] as u16) << 8));

        // Enable RX and TX. We'll set link speed and duplex at link-up.
        self.eth_mac.maccr.write(|w|
            w.re().enabled()
             .te().enabled()
             .cstf().enabled()
        );

        // Tell the ETH DMA the start of each ring
        self.eth_dma.dmatdlar.write(|w| w.stl().bits(self.td.as_ptr() as u32));
        self.eth_dma.dmardlar.write(|w| w.srl().bits(self.rd.as_ptr() as u32));

        // Set DMA bus mode
        self.eth_dma.dmabmr.write(|w|
            w.aab().set_bit()
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
             .cr().cr_150_168()
             .mr().bits(reg)
        );

        // Wait for read
        while self.eth_mac.macmiiar.read().mb().is_busy() {}

        // Return result
        self.eth_mac.macmiidr.read().td().bits()
    }

    /// Write a register over SMI.
    fn smi_write(&mut self, reg: u8, val: u16) {
        // Use PHY address 00000, set write data, set register address, set clock to HCLK/102,
        // start write operation.
        self.eth_mac.macmiidr.write(|w| w.td().bits(val));
        self.eth_mac.macmiiar.write(|w|
            w.mb().busy()
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

/// Store a reference to a TDes. Used to interoperate with smoltcp via AsRef/AsMut.
pub struct TDesRef {
    tdes: *mut TDes,
    eth: *mut EthernetDevice,
}

/// Store a reference to an RDes. Used to interoperate with smoltcp via AsRef.
pub struct RDesRef {
    rdes: *mut RDes,
    eth: *mut EthernetDevice,
}


impl AsRef<[u8]> for TDesRef {
    /// Convert &TDesRef to &[u8] of referenced data automatically, as required by smoltcp
    fn as_ref(&self) -> &[u8] {
        // UNSAFE: Valid TDes will point to valid memory in tdes2 with length in tdes1.
        unsafe {
            core::slice::from_raw_parts((*self.tdes).tdes2 as *const _,
                                        (*self.tdes).tdes1 as usize & 0x1FFF)
        }
    }
}

impl AsMut<[u8]> for TDesRef {
    /// Convert &mut TDesRef to &mut [u8] of referenced data automatically, as required by smoltcp
    fn as_mut(&mut self) -> &mut [u8] {
        // UNSAFE: Valid TDes will point to valid memory in tdes2 with length in tdes1.
        unsafe {
            core::slice::from_raw_parts_mut((*self.tdes).tdes2 as *mut _,
                                            (*self.tdes).tdes1 as usize & 0x1FFF)
        }
    }
}

impl AsRef<[u8]> for RDesRef {
    /// Convert &RDesRef to &[u8] of referenced data automatically, as required by smoltcp
    fn as_ref(&self) -> &[u8] {
        // UNSAFE: Valid RDes will point to valid memory in tdes2 with length in tdes0.
        unsafe {
            core::slice::from_raw_parts((*self.rdes).rdes2 as *const _,
                                        ((*self.rdes).rdes0 >> 16) as usize & 0x3FFF)
        }
    }
}

impl Drop for TDesRef {
    /// When we Drop a TDesRef, tell the Ethernet DMA it's clear to send the packet
    fn drop(&mut self) {
        // Set the length for our TDes and release it to the DMA
        unsafe {
            (*self.tdes).tdes1 = self.as_ref().len() as u32;
            (*self.tdes).tdes0 |= 1<<31;
            (*self.eth).resume_tx_dma();
        }
    }
}

impl Drop for RDesRef {
    /// When we Drop an RDesRef, tell the DMA it now owns that buffer again
    fn drop(&mut self) {
        // Release the buffer back to the DMA
        unsafe {
            (*self.rdes).rdes0 |= 1<<31;
            (*self.eth).resume_rx_dma();
        }
    }
}

// Implement the smoltcp Device interface
impl Device for EthernetDevice {
    type RxBuffer = RDesRef;
    type TxBuffer = TDesRef;

    fn limits(&self) -> DeviceLimits {
        let mut limits = DeviceLimits::default();
        limits.max_transmission_unit = 1500;
        limits.max_burst_size = Some(core::cmp::min(ETH_NUM_TD, ETH_NUM_RD));
        limits
    }

    fn receive(&mut self, _timestamp: u64) -> smoltcp::Result<Self::RxBuffer> {
        // See if the next RDes has been released by the DMA yet and return it if so
        if self.rd[self.rdidx].rdes0 & (1<<31) == 0 {
            let rv = Ok(RDesRef { rdes: &mut self.rd[self.rdidx] as *mut _, eth: self });
            self.rdidx = (self.rdidx + 1) % ETH_NUM_RD;
            return rv;
        } else {
            Err(smoltcp::Error::Exhausted)
        }
    }

    fn transmit(&mut self, _timestamp: u64, length: usize) -> smoltcp::Result<Self::TxBuffer> {
        // See if the next TDes has been released by the DMA yet and return it if so
        if self.td[self.tdidx].tdes0 & (1<<31) == 0 {
            self.td[self.tdidx].tdes1 = length as u32;
            let rv = Ok(TDesRef { tdes: &mut self.td[self.tdidx] as *mut _, eth: self });
            self.tdidx = (self.tdidx + 1) % ETH_NUM_TD;
            return rv;
        } else {
            Err(smoltcp::Error::Exhausted)
        }
    }
}
