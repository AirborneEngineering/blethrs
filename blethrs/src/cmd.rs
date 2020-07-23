use crate::{flash, Error};
use smoltcp::socket::TcpSocket;

/// Information about the build running on the device.
///
/// This can be trivially generated via the `built` crate.
pub struct BuildInfo<'a> {
    pub pkg_version: &'a str,
    pub git_version: &'a str,
    pub built_time_utc: &'a str,
    pub rustc_version: &'a str,
}

pub struct UnknownCommand;

pub const INFO: u32 = 0;
pub const READ: u32 = 1;
pub const ERASE: u32 = 2;
pub const WRITE: u32 = 3;
pub const BOOT: u32 = 4;

/// Read an address and length from the socket
fn read_adr_len(socket: &mut TcpSocket) -> (u32, usize) {
    let mut adr = [0u8; 4];
    let mut len = [0u8; 4];
    socket.recv_slice(&mut adr[..]).ok();
    socket.recv_slice(&mut len[..]).ok();
    let adr = u32::from_le_bytes(adr);
    let len = u32::from_le_bytes(len);
    (adr, len as usize)
}

/// Send a status word back at the start of a response
fn send_status(socket: &mut TcpSocket, status: Error) {
    let resp = (status as u32).to_le_bytes();
    socket.send_slice(&resp).unwrap();
}

/// Read device unique ID, return as array of 24 ASCII hex digits
pub fn get_hex_id() -> [u8; 24] {
    static HEX_DIGITS: [u8; 16] = [
        48, 49, 50, 51, 52, 53, 54, 55, 56, 57,
        65, 66, 67, 68, 69, 70,
    ];
    let id1: [u8; 4] = unsafe { *(0x1FFF_7A10 as *const u32) }.to_le_bytes();
    let id2: [u8; 4] = unsafe { *(0x1FFF_7A14 as *const u32) }.to_le_bytes();
    let id3: [u8; 4] = unsafe { *(0x1FFF_7A18 as *const u32) }.to_le_bytes();
    let id = [
        id3[3], id3[2], id3[1], id3[0],
        id2[3], id2[2], id2[1], id2[0],
        id1[3], id1[2], id1[1], id1[0],
    ];
    let mut out = [0u8; 24];
    for (idx, v) in id.iter().enumerate() {
        let v1 = v & 0x0F;
        let v2 = (v & 0xF0) >> 4;
        out[idx*2  ] = HEX_DIGITS[v2 as usize];
        out[idx*2+1] = HEX_DIGITS[v1 as usize];
    }
    out
}

/// Respond to the information request command with our build information.
pub fn info(build_info: &BuildInfo, socket: &mut TcpSocket) {

    send_status(socket, Error::Success);

    socket.send_slice("blethrs ".as_bytes()).ok();
    socket.send_slice(build_info.pkg_version.as_bytes()).ok();
    socket.send_slice(" ".as_bytes()).ok();
    socket.send_slice(build_info.git_version.as_bytes()).ok();
    socket.send_slice("\r\nBuilt: ".as_bytes()).ok();
    socket.send_slice(build_info.built_time_utc.as_bytes()).ok();
    socket.send_slice("\r\nCompiler: ".as_bytes()).ok();
    socket.send_slice(build_info.rustc_version.as_bytes()).ok();
    socket.send_slice("\r\nMCU ID: ".as_bytes()).ok();
    socket.send_slice(&get_hex_id()).ok();
    socket.send_slice("\r\n".as_bytes()).ok();
}

pub fn read(socket: &mut TcpSocket) {
    let (adr, len) = read_adr_len(socket);
    match flash::read(adr, len) {
        Ok(data) => {
            send_status(socket, Error::Success);
            socket.send_slice(data).unwrap();
        },
        Err(err) => send_status(socket, err),
    };
}

pub fn erase(socket: &mut TcpSocket) {
    let (adr, len) = read_adr_len(socket);
    match flash::erase(adr, len) {
        Ok(()) => send_status(socket, Error::Success),
        Err(err) => send_status(socket, err),
    }
}

pub fn write(socket: &mut TcpSocket) {
    let (adr, len) = read_adr_len(socket);
    match socket.recv(|buf| (buf.len(), flash::write(adr, len, buf))) {
        Ok(Ok(())) => send_status(socket, Error::Success),
        Ok(Err(err)) => send_status(socket, err),
        Err(_) => send_status(socket, Error::NetworkError),
    }
}

pub fn boot(socket: &mut TcpSocket) {
    send_status(socket, Error::Success);
}

/// Respond to the given command.
///
/// Returns whether or not rebooting (via `bootload::reset`) is required.
pub fn handle_and_respond(
    cmd: u32,
    build_info: &BuildInfo,
    socket: &mut TcpSocket,
) -> Result<bool, UnknownCommand> {
    match cmd {
        INFO => info(build_info, socket),
        READ => read(socket),
        ERASE => erase(socket),
        WRITE => write(socket),
        BOOT => {
            boot(socket);
            return Ok(true);
        },
        _ => return Err(UnknownCommand),
    };
    Ok(false)
}
