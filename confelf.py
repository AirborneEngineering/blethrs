#!/usr/bin/env python3

import os
import struct
import argparse
import tempfile
import subprocess
import crcmod


def make_config(mac, ip, gw, prefix):
    magic_bytes = struct.pack("<I", 0x67797870)

    mac_bytes = [int(x, 16) for x in mac.split(":")]
    mac_bytes = struct.pack("BBBBBB", *mac_bytes)

    ip_bytes = [int(x) for x in ip.split(".")]
    ip_bytes = struct.pack("BBBB", *ip_bytes)

    gw_bytes = [int(x) for x in gw.split(".")]
    gw_bytes = struct.pack("BBBB", *gw_bytes)

    prefix_bytes = struct.pack("B", prefix)

    padding_bytes = struct.pack("B", 0)

    config_bytes = magic_bytes + mac_bytes + ip_bytes + gw_bytes + prefix_bytes
    config_bytes += padding_bytes

    crc32 = crcmod.predefined.mkCrcFun('crc-32-mpeg')
    u32 = struct.unpack(">5I", config_bytes)
    raw = struct.pack("<5I", *u32)
    crc = crc32(raw)
    crc_bytes = struct.pack("<I", crc)
    config_bytes += crc_bytes

    return config_bytes


def make_elf(filename, data, address):
    binfile = tempfile.NamedTemporaryFile(suffix=".bin")
    binfile.write(data)
    binfile.flush()

    rawelffile = tempfile.NamedTemporaryFile(suffix=".elf")

    ldfile = tempfile.NamedTemporaryFile("w", suffix=".ld")
    ldfile.write(f"SECTIONS\n{{\n. = 0x{address:08X};\n")
    ldfile.write(".data : { *(.data) }\n}\n")
    ldfile.flush()

    subprocess.run(["arm-none-eabi-ld", "-b", "binary", "-r", "-o",
                    rawelffile.name, binfile.name], check=True)
    subprocess.run(["arm-none-eabi-ld", rawelffile.name, "-T", ldfile.name,
                    "-o", filename], check=True)

    binfile.close()
    rawelffile.close()
    ldfile.close()


def program_elf(filename):
    subprocess.run([
        "arm-none-eabi-gdb", filename, "--batch",
        "-ex", "target extended-remote /dev/ttyACM0",
        "-ex", "monitor swdp_scan",
        "-ex", "attach 1",
        "-ex", "load",
        "-ex", "detach"])


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--lma", default=0x0800C000,
        help="address to save configuration, default 0x0800C000")
    parser.add_argument(
        "--program", action="store_true",
        help="automatically load generated ELF")
    parser.add_argument(
        "mac_address", help="MAC address, in format XX:XX:XX:XX:XX:XX")
    parser.add_argument(
        "ip_address", help="IP address, in format XXX.XXX.XXX.XXX")
    parser.add_argument(
        "gateway_address", help="Gateway address, in format XXX.XXX.XXX.XXX")
    parser.add_argument(
        "prefix_length", type=int, help="Subnet prefix length")
    parser.add_argument(
        "elffile", default="config.elf", help="Output ELF file to write")
    args = parser.parse_args()

    config = make_config(args.mac_address, args.ip_address,
                         args.gateway_address, args.prefix_length)

    make_elf(args.elffile, config, args.lma)

    if args.program:
        program_elf(args.elffile)
        os.remove(args.elffile)


if __name__ == "__main__":
    main()
