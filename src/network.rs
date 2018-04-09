use smoltcp;
use byteorder::{ByteOrder, LittleEndian};

use ::bootload;
use ::flash;

use ethernet::EthernetDevice;

use smoltcp::time::Instant;
use smoltcp::wire::{EthernetAddress, IpAddress, IpCidr};
use smoltcp::iface::{Neighbor, NeighborCache, EthernetInterface, EthernetInterfaceBuilder};
use smoltcp::socket::{SocketSet, SocketSetItem, SocketHandle, TcpSocket, TcpSocketBuffer};

const CMD_INFO: u32 = 0;
const CMD_READ: u32 = 1;
const CMD_ERASE: u32 = 2;
const CMD_WRITE: u32 = 3;
const CMD_BOOT: u32 = 4;

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

pub static mut NETWORK: Network = Network {
    neighbor_cache_storage: [None; 16],
    ip_addr: None,
    eth_iface: None,
    sockets_storage: [None],
    sockets: None,
    tcp_handle: None,
    initialised: false,
};

fn cmd_info(socket: &mut TcpSocket) {
    println!("cmd_info");
    use ::build_info;
    socket.send_slice("blethrs ".as_bytes()).unwrap();
    socket.send_slice(build_info::PKG_VERSION.as_bytes()).unwrap();
    socket.send_slice(" ".as_bytes()).unwrap();
    socket.send_slice(build_info::GIT_VERSION.unwrap().as_bytes()).unwrap();
    socket.send_slice("\r\nPlatform: ".as_bytes()).unwrap();
    socket.send_slice(build_info::TARGET.as_bytes()).unwrap();
    socket.send_slice("\r\nBuilt: ".as_bytes()).unwrap();
    socket.send_slice(build_info::BUILT_TIME_UTC.as_bytes()).unwrap();
    socket.send_slice("\r\nCompiler: ".as_bytes()).unwrap();
    socket.send_slice(build_info::RUSTC_VERSION.as_bytes()).unwrap();
    socket.send_slice("\r\n".as_bytes()).unwrap();
}

fn cmd_adr_len(socket: &mut TcpSocket) -> (u32, usize) {
    let mut adr = [0u8; 4];
    let mut len = [0u8; 4];
    socket.recv_slice(&mut adr[..]).ok();
    socket.recv_slice(&mut len[..]).ok();
    let adr = LittleEndian::read_u32(&adr[..]);
    let len = LittleEndian::read_u32(&len[..]);
    (adr, len as usize)
}

fn cmd_read(socket: &mut TcpSocket) {
    println!("cmd_read");
    let (adr, len) = cmd_adr_len(socket);
    println!("adr={} len={}", adr, len);
    let data = flash::read(adr, len);
    match data {
        Some(data) => { println!("sending data"); socket.send_slice(data).unwrap(); },
        None => println!("error reading"),
    };
}

fn cmd_erase(socket: &mut TcpSocket) {
    println!("cmd_erase");
    let (adr, len) = cmd_adr_len(socket);
    println!("adr={} len={}", adr, len);
    flash::erase(adr, len);
}

fn cmd_write(socket: &mut TcpSocket) {
    println!("cmd_write");
    let (adr, len) = cmd_adr_len(socket);
    println!("adr={} len={}", adr, len);
    socket.recv(|buf| {flash::write(adr, len, buf); (buf.len(), ())}).unwrap();
}

fn cmd_boot() {
    // TODO find a way to defer this so the response can be transmitted
    println!("cmd_boot");
    bootload::reset_bootload();
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

/// Initialise the static NETWORK.
///
/// Sets up the required EthernetInterface and sockets.
pub unsafe fn init<'a>(eth_dev: EthernetDevice, mac_addr: EthernetAddress, ip_addr: IpCidr) {
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
    NETWORK.initialised = true;
}

/// Poll network stack.
///
/// Arrange for this function to be called frequently.
pub fn poll(time_ms: i64) {
    unsafe {
        // Bail out early if NETWORK is not initialised.
        if !NETWORK.initialised {
            return;
        }

        let sockets = NETWORK.sockets.as_mut().unwrap();

        // Handle TCP
        {
            let mut socket = sockets.get::<TcpSocket>(NETWORK.tcp_handle.unwrap());
            if !socket.is_open() {
                socket.listen(7777).unwrap();
            }
            if !socket.may_recv() && socket.may_send() {
                socket.close();
            }
            if socket.can_recv() {
                let mut cmd = [0u8; 4];
                socket.recv_slice(&mut cmd[..]).ok();
                let cmd = LittleEndian::read_u32(&cmd[..]);
                println!("cmd {}", cmd);
                match cmd {
                   CMD_INFO  => cmd_info(&mut socket),
                   CMD_READ => cmd_read(&mut socket),
                   CMD_ERASE => cmd_erase(&mut socket),
                   CMD_WRITE => cmd_write(&mut socket),
                   CMD_BOOT => cmd_boot(),
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
    }
}
