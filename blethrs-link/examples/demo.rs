use blethrs_link as link;
use blethrs_shared::{FLASH_CONFIG, FLASH_USER};
use std::path::Path;
use std::net::{SocketAddr, SocketAddrV4};

// The max number of bytes of a binary file to send via TCP at once.
// Should consider your network's MTU.
const CHUNK_SIZE: usize = 512;

fn main() {
    env_logger::init();

    let mut args = std::env::args();

    let ip_addr_s = args.nth(1).expect("expected IP address string");
    let port_s = args.next().expect("expected port string");
    let cmd_s = args.next().expect("expected command string");

    let addr: SocketAddr = format!("{}:{}", ip_addr_s, port_s)
        .parse::<SocketAddrV4>()
        .expect("failed to parse valid socket address")
        .into();

    println!("Connecting to bootloader...");
    let data = link::info_cmd(&addr).unwrap();
    let s = std::str::from_utf8(&data).unwrap();
    println!("{}", s);

    match &cmd_s[..] {
        "program" => {
            let bin_path_s = args.next().expect("expected path to binary");
            let bin_path = Path::new(&bin_path_s);
            let chunk_size = CHUNK_SIZE;
            let flash_addr = FLASH_USER;
            let bin_data = std::fs::read(&bin_path).unwrap();
            link::write_file(&addr, chunk_size, flash_addr, bin_data).unwrap();
        }
        "configure" => {
            let cfg_flash_addr = FLASH_CONFIG;
            // TODO: These are just for testing - take these via arguments.
            let ip = [10, 101, 0, 1];
            let mac = [0x00, 0x00, 0xAB, 0xCD, ip[2], ip[3]];
            let gw = [ip[0], ip[1], ip[2], 0];
            let prefix = 16;
            link::write_config(&addr, cfg_flash_addr, &mac, &ip, &gw, prefix).unwrap();
        }
        _ => (),
    }

    match &cmd_s[..] {
        "boot" | "program" | "configure" => {
            println!("Sending reboot command...");
            link::boot_cmd(&addr).unwrap();
        }
        _ => (),
    }
}
