use core::fmt::Write;

use smoltcp;
use smoltcp::time::Instant;
use smoltcp::wire::{EthernetAddress, IpAddress, IpCidr};
use smoltcp::iface::{Neighbor, NeighborCache, EthernetInterface, EthernetInterfaceBuilder};
use smoltcp::socket::{SocketSet, SocketSetItem, SocketHandle, TcpSocket, TcpSocketBuffer};

use cortex_m;

use byteorder::{ByteOrder, LittleEndian};

use ::flash;
use ::build_info;
use ::Error;
use ethernet::EthernetDevice;

const CMD_INFO: u32 = 0;
const CMD_READ: u32 = 1;
const CMD_ERASE: u32 = 2;
const CMD_WRITE: u32 = 3;
const CMD_BOOT: u32 = 4;

use ::config::TCP_PORT;

/// Read an address and length from the socket
fn read_adr_len(socket: &mut TcpSocket) -> (u32, usize) {
    let mut adr = [0u8; 4];
    let mut len = [0u8; 4];
    socket.recv_slice(&mut adr[..]).ok();
    socket.recv_slice(&mut len[..]).ok();
    let adr = LittleEndian::read_u32(&adr);
    let len = LittleEndian::read_u32(&len);
    (adr, len as usize)
}

/// Send a status word back at the start of a response
fn send_status(socket: &mut TcpSocket, status: ::Error) {
    let mut resp = [0u8; 4];
    LittleEndian::write_u32(&mut resp, status as u32);
    socket.send_slice(&resp).unwrap();
}

/// Respond to the information request command with our build information.
fn cmd_info(socket: &mut TcpSocket) {

    // Read the device unique ID
    let id1: u32 = unsafe { *(0x1FFF_7A10 as *const u32) };
    let id2: u32 = unsafe { *(0x1FFF_7A14 as *const u32) };
    let id3: u32 = unsafe { *(0x1FFF_7A18 as *const u32) };

    send_status(socket, Error::Success);
    write!(socket, "blethrs {} {}\r\nBuilt: {}\r\nCompiler: {}\r\nMCU ID: {:08X}{:08X}{:08X}\r\n",
           build_info::PKG_VERSION, build_info::GIT_VERSION.unwrap(), build_info::BUILT_TIME_UTC,
           build_info::RUSTC_VERSION, id3, id2, id1).ok();
}

fn cmd_read(socket: &mut TcpSocket) {
    let (adr, len) = read_adr_len(socket);
    match flash::read(adr, len) {
        Ok(data) => {
            send_status(socket, Error::Success);
            socket.send_slice(data).unwrap();
        },
        Err(err) => send_status(socket, err),
    };
}

fn cmd_erase(socket: &mut TcpSocket) {
    let (adr, len) = read_adr_len(socket);
    match flash::erase(adr, len) {
        Ok(()) => send_status(socket, Error::Success),
        Err(err) => send_status(socket, err),
    }
}

fn cmd_write(socket: &mut TcpSocket) {
    let (adr, len) = read_adr_len(socket);
    match socket.recv(|buf| (buf.len(), flash::write(adr, len, buf))) {
        Ok(Ok(())) => send_status(socket, Error::Success),
        Ok(Err(err)) => send_status(socket, err),
        Err(_) => send_status(socket, Error::NetworkError),
    }
}

fn cmd_boot(socket: &mut TcpSocket) {
    send_status(socket, Error::Success);
    ::schedule_reset(50);
}

// Stores the underlying data buffers. If these were included in Network,
// they couldn't live in BSS and therefore take up a load of flash space.
struct NetworkBuffers {
    tcp_tx_buf: [u8; 1536],
    tcp_rx_buf: [u8; 1536],
}

static mut NETWORK_BUFFERS: NetworkBuffers = NetworkBuffers {
    tcp_tx_buf: [0u8; 1536],
    tcp_rx_buf: [0u8; 1536],
};

// Stores all the smoltcp required structs.
pub struct Network<'a> {
    neighbor_cache_storage: [Option<(IpAddress, Neighbor)>; 16],
    ip_addr: Option<[IpCidr; 1]>,
    eth_iface: Option<EthernetInterface<'a, 'a, EthernetDevice>>,
    sockets_storage: [Option<SocketSetItem<'a, 'a>>; 1],
    sockets: Option<SocketSet<'a, 'a, 'a>>,
    tcp_handle: Option<SocketHandle>,
    initialised: bool,
}

static mut NETWORK: Network = Network {
    neighbor_cache_storage: [None; 16],
    ip_addr: None,
    eth_iface: None,
    sockets_storage: [None],
    sockets: None,
    tcp_handle: None,
    initialised: false,
};

/// Initialise the static NETWORK.
///
/// Sets up the required EthernetInterface and sockets.
///
/// Do not call more than once or this function will panic.
pub fn init<'a>(eth_dev: EthernetDevice, mac_addr: EthernetAddress, ip_addr: IpCidr) {
    // Unsafe required for access to NETWORK.
    // NETWORK.initialised guards against calling twice.
    unsafe {
        cortex_m::interrupt::free(|_| {
            if NETWORK.initialised {
                panic!("NETWORK already initialised");
            }
            NETWORK.initialised = true;
        });

        let neighbor_cache = NeighborCache::new(&mut NETWORK.neighbor_cache_storage.as_mut()[..]);

        NETWORK.ip_addr = Some([ip_addr]);
        NETWORK.eth_iface = Some(EthernetInterfaceBuilder::new(eth_dev)
                                .ethernet_addr(mac_addr)
                                .neighbor_cache(neighbor_cache)
                                .ip_addrs(&mut NETWORK.ip_addr.as_mut().unwrap()[..])
                                .finalize());

        NETWORK.sockets = Some(SocketSet::new(&mut NETWORK.sockets_storage.as_mut()[..]));
        let tcp_rx_buf = TcpSocketBuffer::new(&mut NETWORK_BUFFERS.tcp_rx_buf.as_mut()[..]);
        let tcp_tx_buf = TcpSocketBuffer::new(&mut NETWORK_BUFFERS.tcp_tx_buf.as_mut()[..]);
        let tcp_socket = TcpSocket::new(tcp_rx_buf, tcp_tx_buf);
        NETWORK.tcp_handle = Some(NETWORK.sockets.as_mut().unwrap().add(tcp_socket));
    }
}

/// Poll network stack.
///
/// Arrange for this function to be called frequently.
pub fn poll(time_ms: i64) {
    // Unsafe required to access static mut NETWORK.
    // Since the entire poll is run in an interrupt-free context no
    // other access to NETWORK can occur.
    cortex_m::interrupt::free(|_| unsafe {
        // Bail out early if NETWORK is not initialised.
        if !NETWORK.initialised {
            return;
        }

        let sockets = NETWORK.sockets.as_mut().unwrap();

        // Handle TCP
        {
            let mut socket = sockets.get::<TcpSocket>(NETWORK.tcp_handle.unwrap());
            if !socket.is_open() {
                socket.listen(TCP_PORT).unwrap();
            }
            if !socket.may_recv() && socket.may_send() {
                socket.close();
            }
            if socket.can_recv() {
                let mut cmd = [0u8; 4];
                socket.recv_slice(&mut cmd[..]).ok();
                let cmd = LittleEndian::read_u32(&cmd[..]);
                match cmd {
                   CMD_INFO  => cmd_info(&mut socket),
                   CMD_READ => cmd_read(&mut socket),
                   CMD_ERASE => cmd_erase(&mut socket),
                   CMD_WRITE => cmd_write(&mut socket),
                   CMD_BOOT => cmd_boot(&mut socket),
                    _ => (),
                };
                socket.close();
            }
        }

        // Poll smoltcp
        let timestamp = Instant::from_millis(time_ms);
        match NETWORK.eth_iface.as_mut().unwrap().poll(sockets, timestamp) {
            Ok(_) | Err(smoltcp::Error::Exhausted) => (),
            Err(_) => (),
        }
    });
}
