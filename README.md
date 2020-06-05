# phy_channels
Rust Phoenix Channel client wrapper for Python

This is still experimental, credits to https://github.com/tshakah/phoenix-channels-rs and https://github.com/gsterjov/phoenix-channels-rs for the initial work.

How to build:
* install rust nightly
* git clone https://github.com/rizki96/phy_channels
* cargo build
* cp target/debug/libphy_channels.dylib package/phy_channels/phy_channels.so

How to use:
* example code is in 'package/test.py'
* cd package
* python test.py
