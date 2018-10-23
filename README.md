# blethrs

An ethernet bootloader for STM32s written in Rust,
using the [smoltcp](https://github.com/m-labs/smoltcp) TCP/IP stack.

## Building


    cargo build --release

The resulting executable is at `target/thumbv7em-none-eabihf/release/blethrs`


## Default Config

Without a valid config in flash, blethers defaults to IP address `10.1.1.10`,
gateway `10.1.1.1`, MAC address `02:00:01:02:03:04`.

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.

## Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in the work by you, as defined in the Apache-2.0 license, shall be
dual licensed as above, without any additional terms or conditions.
