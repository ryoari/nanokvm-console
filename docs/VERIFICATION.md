# Verification boundary

Cargo invokes the pinned Lean toolchain. A failed Lean build stops the Rust build.
The Rust executable includes source digests and the expected guard digest. At
runtime it requires the matching `console_guard` beside the executable. Source
files are build provenance, not runtime dependencies. Editing source requires a
new build to change the installed behavior.

`verification/Console.lean` defines the input state machine and wire encoder.
Its proofs require an armed connection and valid bounds for keyboard/pointer
reports. Reconnection starts disarmed. Close and disarm produce neutral reports
when the model says input is armed. The server uses this executable for live
reports; it does not maintain a second input encoder.

`verification/Evidence.lean` checks a structured observation contract for video,
keyboard, pointer and reconnection evidence, including active target observations
and an empty held-key list. Synthetic positive and negative cases run in the test suite. It cannot attest that submitted
observations came from hardware. Keep hardware observations separate from
synthetic fixtures.

These proofs do not verify the browser, Rust compiler, TLS stack, device firmware,
USB delivery, physical input effects, network availability or power stability.
Rust tests and isolated browser tests exercise those software boundaries where
possible. Physical effects require the dedicated target test.
