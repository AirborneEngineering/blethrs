import time
import struct
import socket
import argparse
import crcmod


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
    4: "Erase Error",
    5: "Write Error",
    6: "Flash Error",
    7: "Network Error",
    8: "Internal Error",
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


def boot_request(hostname, port):
    print("Sending UDP boot request to port {}...".format(port))
    s = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    cmd = struct.pack("<I", 28)
    s.sendto(cmd, (hostname, port))
    print("Sent, waiting for reboot...")
    time.sleep(2)


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


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("hostname", help="IP address/hostname of bootloader")
    parser.add_argument("mac_address",
                        help="MAC address, in format XX:XX:XX:XX:XX:XX")
    parser.add_argument("ip_address",
                        help="IP address, in format XXX.XXX.XXX.XXX")
    parser.add_argument("gateway_address",
                        help="Gateway address, in format XXX.XXX.XXX.XXX")
    parser.add_argument("prefix_length", type=int, help="Subnet prefix length")
    parser.add_argument("--port", type=int, default=7777,
                        help="bootloader port, default 7777")
    parser.add_argument("--lma", default=0x0800C000,
                        help="address to write to, default 0x0800C000")
    parser.add_argument("--boot-req", action='store_true',
                        help="send an initial boot request to user firmware")
    parser.add_argument("--boot-req-port", type=int, default=1735,
                        help="UDP port for boot request, default 1735")
    parser.add_argument("--bootload", action='store_true',
                        help="send a reboot command after writing config")
    args = parser.parse_args()

    config_bytes = make_config(args.mac_address, args.ip_address,
                               args.gateway_address, args.prefix_length)

    if args.boot_req:
        boot_request(args.hostname, args.boot_req_port)

    try:
        print("Connecting to bootloader...")
        info = info_cmd(args.hostname, args.port)
        print("Received bootloader information:")
        print(info.decode())

        print("Erasing old configuration...")
        erase_cmd(args.hostname, args.port, args.lma, len(config_bytes))

        print("Writing new configuration...")
        write_cmd(args.hostname, args.port, args.lma, config_bytes)

        print("Reading back new configuration...")
        rdata = read_cmd(args.hostname, args.port, args.lma, len(config_bytes))

        if config_bytes != rdata:
            for idx in range(len(config_bytes)):
                if config_bytes[idx] != rdata[idx]:
                    raise MismatchError(
                        args.lma + idx, config_bytes[idx], rdata[idx])

        print("Readback successful.")

        if args.bootload:
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
