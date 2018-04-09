extern crate smoltcp;

use ::bootload;
use ::flash;

use ethernet::EthernetDevice;

use self::smoltcp::time::Instant;
use self::smoltcp::wire::{EthernetAddress, IpAddress, IpCidr};
use self::smoltcp::iface::{Neighbor, NeighborCache, EthernetInterface, EthernetInterfaceBuilder};
use self::smoltcp::socket::{SocketSet, SocketSetItem, SocketHandle, TcpSocket, TcpSocketBuffer};

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
                socket.recv(|buf| (buf.len(), ())).unwrap();
                let resp = "bootloading...\r\n".as_bytes();
                socket.send_slice(resp).unwrap();
                socket.close();

                // TODO find a way to defer this so the response can be transmitted
                bootload::reset_bootload();
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
