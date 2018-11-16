#!/bin/bash
set -exuo pipefail

arm-none-eabi-gdb target/thumbv7em-none-eabihf/release/blethrs --batch \
    -ex "target extended-remote /dev/ttyACM0" \
    -ex "monitor swdp_scan" \
    -ex "attach 1" \
    -ex "load" \
    -ex "start" \
    -ex "detach"
