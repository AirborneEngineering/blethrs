# blethrs

An ethernet bootloader for STM32s written in Rust,
using the [smoltcp](https://github.com/m-labs/smoltcp) TCP/IP stack.

## License

blethrs is distributed under the terms of both the MIT and Apache 2.0 license.

## Building


    cargo build --release

The resulting executable is at `target/thumbv7em-none-eabihf/release/blethrs`


## Default Config

Without a valid config in flash, blethers defaults to IP address `10.1.1.10`,
gateway `10.1.1.1`, MAC address `02:00:01:02:03:04`.

