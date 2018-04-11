import time
import struct
import socket
import argparse

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
    def __init__(self, addr):
        self.addr = addr

    def __str__(self):
        return "Mismatch at address {:08X}".format(self.addr)


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


def write_file(hostname, port, address, data):
    length = len(data)
    segments = length // 1024
    if length % 1024 != 0:
        segments += 1

    print("Erasing (may take a few seconds)...")
    erase_cmd(hostname, port, address, length)

    print("Writing {:.02f}kB in {} segments...".format(length/1024, segments))
    for sidx in tqdm(range(segments), unit='kB'):
        saddr = address + sidx*1024
        sdata = data[sidx*1024:(sidx+1)*1024]
        write_cmd(hostname, port, saddr, sdata)

    print("Writing completed successfully. Reading back...")
    for sidx in tqdm(range(segments), unit='kB'):
        saddr = address + sidx*1024
        sdata = data[sidx*1024:(sidx+1)*1024]
        rdata = read_cmd(hostname, port, saddr, 1024)
        if sdata != rdata[:len(sdata)]:
            for idx in range(len(sdata)):
                if sdata[idx] != rdata[idx]:
                    raise MismatchError(saddr + idx, sdata[idx], rdata[idx])
    print("Readback successful.")


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("hostname", help="IP address/hostname of bootloader")
    parser.add_argument("binfile", type=argparse.FileType('rb'),
                        help="raw binary file to program")
    parser.add_argument("--port", type=int, default=7777,
                        help="bootloader port, default 7777")
    parser.add_argument("--lma", default=0x08010000,
                        help="address to load to, default 0x08010000")
    parser.add_argument("--boot-req", action='store_true',
                        help="send an initial boot request to user firmware")
    parser.add_argument("--boot-req-port", type=int, default=1735,
                        help="UDP port for boot request, default 1735")
    args = parser.parse_args()

    bindata = args.binfile.read()
    padding = len(bindata) % 4
    bindata += b"\x00"*padding

    if args.boot_req:
        boot_request(args.hostname, args.boot_req_port)

    try:
        print("Connecting to bootloader...")
        info = info_cmd(args.hostname, args.port)
        print("Received bootloader information:")
        print(info.decode())

        write_file(args.hostname, args.port, args.lma, bindata)

        print("Booting new firmware...")
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
