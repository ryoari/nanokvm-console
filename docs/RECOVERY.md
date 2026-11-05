# Connection recovery

Configure routes in order of preference during pairing. A route must already be
reachable from the operator. The app does not change network settings or create a
VPN. USB networking can work without Wi-Fi when the operator has that USB route.

A failed TCP connection permits the next route. A certificate, TLS, authentication
or device-identity failure stops recovery. Resolve that fault before retrying.
A second IP address is not permission to control a different device.

After an unexpected console transport closure, the browser tries again after
1, 3 and 8 seconds. Each connection attempt also has network timeouts. Select
**Close console** to cancel. The retry count resets on a manual Connect action.
The app does not re-enable input. Unconfirmed release of input stops automatic retry. Check the physical target before a manual reconnect. A stalled video peer gets 10 seconds to recover
before its transport is closed. An unconfirmed close keeps its ownership state;
close that tab and reopen the console from the launcher.

## Distinguish a reboot from a disconnect

- A repeated boot logo on the physical device indicates a device restart.
- **Check connection** reports the Linux boot ID and uptime in Technical details.
- A changed boot ID between checks confirms a reboot in that interval.
- A browser disconnect alone does not establish a reboot.
- The first check has no previous boot to compare. Checks do not establish the
  reboot cause, and the app does not preserve a full device log.

If the device stays up but a route fails, inspect the USB network interface,
address and routing on the operator. Retry the configured alternate route.
For a separate operator, a route must exist across the networks involved.

If the physical device repeatedly restarts, record the preceding device-screen
message and inspect its network, application and boot logs. A reconnect message
before a crash warrants investigation of the network retry path. Power is one
possible cause; the message alone cannot establish it.
Sipeed identifies insufficient or unstable power as a common cause and recommends
a separate USB-PD supply and suitable cable. Disconnecting an auxiliary PD supply
can itself cause a restart. Make power changes only in a planned maintenance
window. [Vendor boot guidance](https://wiki.sipeed.com/hardware/en/kvm/NanoKVM_Go/faq.html#boot-issues)

This app never reboots, resets, flashes or power-cycles the device. It does not
change Wi-Fi settings. Software retry cannot repair a physical power fault.

A public [Wi-Fi retry report](https://github.com/sipeed/NanoKVM-Go/issues/18)
describes an unavailable saved network leaving the device in a connection loop.
A separate [Go+ reboot report](https://github.com/sipeed/NanoKVM-Go/issues/17)
concerns application 0.2.2. These reports do not establish a common cause or a
verified fix. Use the USB route to inspect the device when Wi-Fi is unavailable.
Do not flash or reset it as an automatic recovery action.
