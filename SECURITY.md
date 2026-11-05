# Security scope

The server accepts only its loopback Host and Origin. A private launch capability
creates an HttpOnly, SameSite cookie. Local WebSocket input also requires the
session token. Keep state files and connection profiles private.

The saved device certificate is the only TLS trust root. The client checks the
peer certificate before login and checks the device mDNS identity after login.
The vendor certificate can lack a matching hostname; hostname validation is
replaced by saved certificate trust. There are no redirect or proxy fallbacks.
Authentication and trust failures stop route recovery.

Anyone with control of the local account can read its device profile and replace
the executable. This app is not a sandbox against that account. Do not share the
profile, launch link, diagnostics or target recordings. Use the vendor's private
network features for remote access rather than exposing this local listener.

Report defects without credentials, screenshots of private screens, or raw device
logs. Include software versions, a synthetic reproduction and the failed behavior.
