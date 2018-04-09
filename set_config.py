import time
import struct
import socket
import crcmod
import argparse


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("hostname")
    parser.add_argument("port", type=int)
    parser.add_argument("mac_address")
    parser.add_argument("ip_address")
    parser.add_argument("gateway_address")
    parser.add_argument("prefix_length")
    parser.add_argument("--config_address", default="0x0800C000")
    args = parser.parse_args()

    mac_bytes = [int(x, 16) for x in args.mac_address.split(":")]
    mac_bytes = struct.pack("BBBBBB", *mac_bytes)

    ip_bytes = [int(x) for x in args.ip_address.split(".")]
    ip_bytes = struct.pack("BBBB", *ip_bytes)

    gw_bytes = [int(x) for x in args.gateway_address.split(".")]
    gw_bytes = struct.pack("BBBB", *gw_bytes)

    prefix_bytes = struct.pack("B", int(args.prefix_length))

    magic_bytes = struct.pack("<I", 0x67797870)
    padding_bytes = struct.pack("B", 0)

    config_bytes = magic_bytes + mac_bytes + ip_bytes + gw_bytes + prefix_bytes
    config_bytes += padding_bytes
    crc32 = crcmod.predefined.mkCrcFun('crc-32-mpeg')
    u32 = struct.unpack(">5I", config_bytes)
    raw = struct.pack("<5I", *u32)
    crc = crc32(raw)
    crc_bytes = struct.pack("<I", crc)
    config_bytes += crc_bytes

    conf_address = int(args.config_address, 16)
    erase_cmd = struct.pack("<III", 2, conf_address, 24)
    write_cmd = struct.pack("<III", 3, conf_address, 24) + config_bytes

    print("Erasing...")
    s = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    s.connect((args.hostname, args.port))
    s.sendall(erase_cmd)
    s.recv(1024)
    s.close()

    time.sleep(2)

    print("Writing...")
    s = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    s.connect((args.hostname, args.port))
    s.sendall(write_cmd)
    s.recv(1024)
    s.close()


if __name__ == "__main__":
    main()
