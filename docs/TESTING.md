# Repeatable console test

Use only the dedicated test screen and synthetic text. Keep it full screen and
active on the target. Stop if the target window loses focus.

1. Open `tools/test-screen.html` on the target and select **Enter full screen**.
2. Open the console on the operator and select **Check connection**.
3. Record the device model, image/application versions, selected route and uptime
   locally. Do not publish a raw diagnostic or launch file.
4. Connect with input disabled. Confirm red, green and blue blocks. Confirm that
   the clock changes. Record decoded frame counts from two connection checks.
5. Enable input. Click the text field through the video and type `kvmtest`.
6. Check the target directly. It must show exactly `kvmtest`, seven key presses,
   seven key releases and an empty held-key list. This is confirmation independent
   of the sender.
7. Click the mouse test area through the video. Check the target directly for one
   added click and the expected coordinates.
8. Disable input. Close the software console. Reconnect without touching cables
   or power. Confirm moving video and **Input disabled**.
9. Repeat the keyboard and mouse checks if the configuration changed.
10. Close the console and test window. Keep any captures local.

Network and power interruptions need a separate controlled test. The automated
software suite uses loopback TLS fixtures and synthetic browser peers. It does not
prove hardware failover, power redundancy or recovery from a physical reboot.

## Software checks

Install Python test dependencies in a virtual environment:

```sh
python3 -m venv .venv
.venv/bin/pip install -r requirements-test.txt
.venv/bin/python -m playwright install chromium
cargo test --locked
cargo build --release --locked
.venv/bin/python -m pytest tests -q
```

Set `CHROME` to an installed Chromium or Chrome executable if needed. Browser
tests use an isolated headless profile. They do not access a personal profile or
send hardware input. Tests require `openssl` and loopback sockets.
