#!/usr/bin/env python3

import time
import struct
import socket
import argparse
import crcmod

try:
    from tqdm import tqdm
except ImportError:
    print("Notice: tqdm not installed, install for progress bars.")

    def tqdm(x, *args, **kwargs):
        return x


commands = {
    "info": 0,
    "read": 1,
    "erase": 2,
    "write": 3,
    "boot": 4,
}


errors = {
    0: "Success",
    1: "Invalid Address",
    2: "Length Not Multiple of 4",
    3: "Length Too Long",
    4: "Data Length Incorrect",
    5: "Erase Error",
    6: "Write Error",
    7: "Flash Error",
    8: "Network Error",
    9: "Internal Error",
}


class BootloaderError(Exception):
    def __init__(self, errno):
        self.errno = errno

    def __str__(self):
        if self.errno in errors:
            return "{}".format(errors[self.errno])
        else:
            return "Unknown error {}".format(self.errno)


class MismatchError(Exception):
    def __init__(self, addr, tx, rx):
        self.addr = addr
        self.tx = tx
        self.rx = rx

    def __str__(self):
        return "Mismatch at address {:08X}: {:02X}!={:02X}".format(
            self.addr, self.tx, self.rx)


def boot_request(hostname, boot_req_port, bootloader_port, n_attempts=10):
    print("Sending UDP boot request to port {}...".format(boot_req_port))
    s = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    cmd = struct.pack("<I", 28)
    s.sendto(cmd, (hostname, boot_req_port))
    print("Sent, waiting for reboot...")

    # We wait half a second then attempt TCP connection to the bootloader,
    # and retry up to n_attempts times before raising the conection error
    # back to the main loop. This gives the bootloader time to boot and
    # establish the network link.
    cmd = struct.pack("<I", commands['info'])
    for attempt in range(n_attempts):
        try:
            time.sleep(0.5)
            interact(hostname, bootloader_port, cmd, timeout=0.5)
        except OSError as e:
            if attempt == n_attempts - 1:
                raise e
        else:
            break


def interact(hostname, port, command, timeout=2):
    s = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    s.settimeout(timeout)
    s.connect((hostname, port))
    s.sendall(command)
    data = check_response(s.recv(2048))
    s.close()
    time.sleep(0.01)
    return data


def check_response(data):
    errno = struct.unpack("<I", data[:4])[0]
    if errno != 0:
        raise BootloaderError(errno)
    return data[4:]


def info_cmd(hostname, port):
    cmd = struct.pack("<I", commands['info'])
    return interact(hostname, port, cmd)


def erase_cmd(hostname, port, address, length):
    cmd = struct.pack("<III", commands['erase'], address, length)
    interact(hostname, port, cmd, timeout=20.0)


def read_cmd(hostname, port, address, length):
    cmd = struct.pack("<III", commands['read'], address, length)
    return interact(hostname, port, cmd)


def write_cmd(hostname, port, address, data):
    cmd = struct.pack("<III{}B".format(len(data)), commands['write'],
                      address, len(data), *data)
    interact(hostname, port, cmd)


def boot_cmd(hostname, port):
    cmd = struct.pack("<I", commands['boot'])
    interact(hostname, port, cmd)


def write_file(hostname, port, chunk_size, address, data):
    # We need to write in multiples of 4 bytes (since writes are word-by-word),
    # so add padding to the end of the data.
    length = len(data)
    if length % 4 != 0:
        padding = 4 - length % 4
        data += b"\xFF"*padding
        length += padding
    segments = length // chunk_size
    if length % chunk_size != 0:
        segments += 1

    print("Erasing (may take a few seconds)...")
    erase_cmd(hostname, port, address, length)

    print("Writing {:.02f}kB in {} segments...".format(length/1024, segments))
    for sidx in tqdm(list(reversed(range(segments))),
                     unit='kB', unit_scale=chunk_size/1024):
        saddr = address + sidx*chunk_size
        sdata = data[sidx*chunk_size:(sidx+1)*chunk_size]
        write_cmd(hostname, port, saddr, sdata)

    print("Writing completed successfully. Reading back...")
    for sidx in tqdm(range(segments), unit='kB', unit_scale=chunk_size/1024):
        saddr = address + sidx*chunk_size
        sdata = data[sidx*chunk_size:(sidx+1)*chunk_size]
        rdata = read_cmd(hostname, port, saddr, chunk_size)
        if sdata != rdata[:len(sdata)]:
            for idx in range(len(sdata)):
                if sdata[idx] != rdata[idx]:
                    raise MismatchError(saddr + idx, sdata[idx], rdata[idx])
    print("Readback successful.")


def write_config(hostname, port, address, mac, ip, gw, prefix):
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

    print("Erasing old configuration...")
    erase_cmd(hostname, port, address, len(config_bytes))

    print("Writing new configuration...")
    write_cmd(hostname, port, address, config_bytes)

    print("Reading back new configuration...")
    rdata = read_cmd(hostname, port, address, len(config_bytes))

    if config_bytes != rdata:
        for idx in range(len(config_bytes)):
            if config_bytes[idx] != rdata[idx]:
                raise MismatchError(
                    address + idx, config_bytes[idx], rdata[idx])

    print("Readback successful.")


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("hostname", help="IP address/hostname of bootloader")
    parser.add_argument("--port", type=int, default=7777,
                        help="bootloader port, default 7777")
    parser.add_argument("--boot-req", action='store_true',
                        help="send an initial boot request to user firmware")
    parser.add_argument("--boot-req-port", type=int, default=1735,
                        help="UDP port for boot request, default 1735")
    parser.add_argument("--no-reboot", action='store_true',
                        help="don't send a reboot request after completion")
    parser.add_argument("--chunk-size", type=int, default=512,
                        help="Size of chunks to write to flash, default 512")
    subparsers = parser.add_subparsers(dest="command")
    subparsers.required = True
    subparsers.add_parser(
        "info", help="Just read bootloader information without rebooting")
    parser_program = subparsers.add_parser(
        "program", help="Bootload new firmware image")
    parser_program.add_argument("--lma", default=0x08010000,
                                help="address to load to, default 0x08010000")
    parser_program.add_argument("binfile", type=argparse.FileType('rb'),
                                help="raw binary file to program")
    parser_configure = subparsers.add_parser(
        "configure", help="Load new configuration")
    parser_configure.add_argument(
        "--lma", default=0x0800C000,
        help="address to write to, default 0x0800C000")
    parser_configure.add_argument(
        "mac_address", help="MAC address, in format XX:XX:XX:XX:XX:XX")
    parser_configure.add_argument(
        "ip_address", help="IP address, in format XXX.XXX.XXX.XXX")
    parser_configure.add_argument(
        "gateway_address", help="Gateway address, in format XXX.XXX.XXX.XXX")
    parser_configure.add_argument(
        "prefix_length", type=int, help="Subnet prefix length")
    subparsers.add_parser("boot", help="Send immediate reboot request")
    args = parser.parse_args()
    cmd = args.command

    try:
        if args.boot_req:
            boot_request(args.hostname, args.boot_req_port, args.port)

        print("Connecting to bootloader...")
        info = info_cmd(args.hostname, args.port)
        print("Received bootloader information:")
        print(info.decode())

        if cmd == "program":
            bindata = args.binfile.read()
            write_file(args.hostname, args.port, args.chunk_size, args.lma,
                       bindata)
        elif cmd == "configure":
            write_config(args.hostname, args.port, args.lma,
                         args.mac_address, args.ip_address,
                         args.gateway_address, args.prefix_length)

        if cmd == "boot" or (not args.no_reboot and cmd != "info"):
            print("Sending reboot command...")
            boot_cmd(args.hostname, args.port)

    except OSError as e:
        print("Connection error:", e)
        print("Check hostname is correct and device is in bootloader mode.")
    except BootloaderError as e:
        print("Bootloader error:", e)
    except MismatchError as e:
        print("Mismatch error:", e)


if __name__ == "__main__":
    main()
