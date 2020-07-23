use crate::UserConfig;
use ufmt::uwrite;

fn u8_to_hex(x: u8, buf: &mut [u8]) -> &str {
    static HEX_DIGITS: [u8; 16] = [
        48, 49, 50, 51, 52, 53, 54, 55, 56, 57,
        65, 66, 67, 68, 69, 70,
    ];
    let v1 = x & 0x0F;
    let v2 = (x & 0xF0) >> 4;
    buf[0] = HEX_DIGITS[v2 as usize];
    buf[1] = HEX_DIGITS[v1 as usize];
    unsafe { core::str::from_utf8_unchecked(buf) }
}

impl ufmt::uDisplay for UserConfig {
    fn fmt<W: ?Sized>(&self, f: &mut ufmt::Formatter<W>) -> core::result::Result<(), W::Error>
    where
        W: ufmt::uWrite,
    {
        let mut hexbuf = [0u8; 2];
        uwrite!(f, "  MAC Address: ",)?;
        uwrite!(f, "{}:", u8_to_hex(self.mac_address[0], &mut hexbuf))?;
        uwrite!(f, "{}:", u8_to_hex(self.mac_address[1], &mut hexbuf))?;
        uwrite!(f, "{}:", u8_to_hex(self.mac_address[2], &mut hexbuf))?;
        uwrite!(f, "{}:", u8_to_hex(self.mac_address[3], &mut hexbuf))?;
        uwrite!(f, "{}:", u8_to_hex(self.mac_address[4], &mut hexbuf))?;
        uwrite!(f, "{}\n", u8_to_hex(self.mac_address[5], &mut hexbuf))?;
        uwrite!(f, "  IP Address: {}.{}.{}.{}/{}\n",
               self.ip_address[0], self.ip_address[1], self.ip_address[2], self.ip_address[3],
               self.ip_prefix)?;
        uwrite!(f, "  Gateway: {}.{}.{}.{}\n",
               self.ip_gateway[0], self.ip_gateway[1], self.ip_gateway[2],
               self.ip_gateway[3])?;
        uwrite!(f, "  Checksum: {}\n", self.checksum as u32)
    }
}
