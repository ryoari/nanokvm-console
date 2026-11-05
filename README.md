# NanoKVM console

A small local browser console for NanoKVM Go+. The server is Rust. A Lean 4
program checks and encodes keyboard and mouse reports. Video uses the device's
H.264 WebRTC interface. No cloud service is required.

- Four controls: connect, enable input, close, and check connection.
- Ordered routes to one trusted device, with USB first when available.
- Up to three recovery attempts after a transport loss. Input stays disabled.
- Connection, capture, certificate, version, uptime and reboot diagnostics.
- Private local launch links. Credentials stay in a mode-600 local profile.

## Connect the hardware

The **operator computer** runs this app. The **target computer** supplies the
screen and receives keyboard/mouse input. The target does not need the app.
NanoKVM Go+ connects to the target through its full-featured USB-C data/video
connection. The target port and cable must support DisplayPort Alt Mode.
See the [vendor quick start](https://wiki.sipeed.com/hardware/en/kvm/NanoKVM_Go/quick_start.html)
for the exact ports and compatible equipment.

For a local test on one computer, use the device's USB network address. For a
separate operator, configure a reachable LAN route or an existing private network
route. A USB address on the target is not automatically reachable from a different
operator computer. Joining different Wi-Fi networks does not create a route.

Keep the power supply stable. A fallback network route cannot prevent a hardware
reboot. See [recovery](docs/RECOVERY.md).

## Install on Linux

Install Git, a C toolchain, OpenSSL development headers, pkg-config, Python 3 on
the device (already present in the tested image), and xdg-utils on the operator.
On Debian or Ubuntu, the operator packages are:

```sh
sudo apt install git build-essential pkg-config libssl-dev xdg-utils
```

Install [Rust](https://www.rust-lang.org/tools/install) and
[elan](https://github.com/leanprover/elan#installation). Add their bin directories
to `PATH`. The repository pins Lean in `verification/lean-toolchain`.

```sh
git clone https://github.com/ryoari/nanokvm-console.git
cd nanokvm-console
./scripts/install.sh
export PATH="$HOME/.local/bin:$PATH"
nanokvm-console --verify
```

The installer builds with the lockfile. It installs the Rust executable beside
its matching Lean guard. They can run without the source checkout. Use `PREFIX`
to select another installation root. Do not use `cargo install` alone: it does
not install the guard. No firmware installation occurs.

## Pair once

Get the device's HTTPS address and mDNS name from its screen or vendor console.
Obtain its certificate over a trusted local connection. For example, replace
`DEVICE_ADDRESS` and `DEVICE_NAME` below with your device's values:

```sh
config_dir="${XDG_CONFIG_HOME:-$HOME/.config}/nanokvm-console"
mkdir -p "$config_dir"
openssl s_client -connect DEVICE_ADDRESS:443 -servername DEVICE_NAME -showcerts </dev/null 2>/dev/null |
  openssl x509 -out "$config_dir/device.pem"
nanokvm-console --pair --endpoint https://DEVICE_ADDRESS --device DEVICE_NAME \
  --certificate "$config_dir/device.pem"
```

This first certificate retrieval is trust on first use. It does not establish
identity over an untrusted network. Confirm the physical device and trusted
connection before accepting the displayed fingerprint. Pairing asks for the
username and a hidden password. It does not overwrite a saved profile.

Repeat `--endpoint https://ANOTHER_ADDRESS` during pairing to add up to two
fallback routes to the **same device**. Put USB first when it is reachable from
the operator. Every route uses the saved certificate and device identity.
Use `--profile /absolute/path/connection.json` for a separate saved profile.
Never put a connection profile in this repository.

## Open and use

```sh
nanokvm-console --open
```

1. Select **Check connection**. This runs a bounded, read-only metadata probe through the device terminal.
2. Stop Memory Fabric in the vendor console if the check requires it.
3. Select **Connect / reconnect**. This selects H.264 WebRTC mode on the device.
4. Confirm that the remote screen is current.
5. Select **Enable input**.
6. Click the video before typing.
7. Press Escape to disable input.
8. Select **Close console** to close the software session.

Input starts disabled after every connection. Losing video or window focus also
disables it. A network failure can prevent release reports from reaching the
physical target; local disarm is not proof of physical release. Never assume a
lost connection has released a held key. Check the target after recovery.
Ctrl+C stops the server. The app binds only to `127.0.0.1`; do not expose this
listener to a network. Use the private `launch.html` in the state directory if a
browser cannot open automatically. The default state directory is
`$HOME/.local/state/nanokvm-console`.

## Verify on a target

Open [tools/test-screen.html](tools/test-screen.html) on the target. Enter full
screen. Keep that window active. Then follow [the repeatable test](docs/TESTING.md).
Use synthetic content for all captures and input tests.

The supported baseline is Go+ image v1.0.9, application 0.2.2, a Linux operator,
and Chromium. Other firmware versions, operating systems, WAN routes and normal
NanoKVM Go need separate qualification. Go and Go+ are not treated as interchangeable.

Hardware checks on that baseline confirmed 1920 × 1200 video, keyboard and mouse
effects in a separate receiver window, software closure/reconnection, and fallback
from an unavailable route to USB. The operator and target were the same Linux
computer. Automatic retry timing and trust-failure cases use synthetic tests.
The Wi-Fi-related physical reboot report remains unresolved; this app does not
claim to repair that device fault.

See [verification limits](docs/VERIFICATION.md) and [security](SECURITY.md).
Licensed under [GPL-3.0-or-later](LICENSE). This is an independent client, not a
Sipeed product.
