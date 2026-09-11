## What's new

### Lossless FLAC restored on Windows

Windows now streams and downloads **lossless FLAC** again. This works by replicating the Tidal desktop app's PKCE auth flow, which issues tokens with full lossless access. On login, a browser window opens automatically — just log in and Lumitide handles the rest.

Linux now uses the same PKCE flow: on login a browser opens automatically and Lumitide temporarily registers itself as the `tidal://` handler via `xdg-mime`, catching the redirect and restoring the previous handler afterwards. macOS and headless Linux (no `xdg-mime`) keep the device code flow and **MP4 (HIGH quality / AAC)**. Run `lumitide login` to re-authorise an existing session and upgrade it to lossless.

### What changed under the hood

- **New auth flow (Windows):** PKCE OAuth with the Tidal desktop client — browser opens automatically, OS redirects back to Lumitide, no copy-pasting needed
- **New auth flow (Linux):** same PKCE flow — Lumitide registers a temporary `tidal://` scheme handler (desktop entry + `xdg-mime`), catches the browser redirect, then restores the previous handler
- **`lumitide login`:** re-run the login flow to replace an existing session (e.g. to upgrade a device-code session to lossless PKCE tokens)
- **AES-128-CTR stream decryption:** lossless streams are encrypted at rest; Lumitide now unwraps the per-track key and decrypts on the fly during download
- **Client-aware sessions and refresh:** sessions record which OAuth client issued them; token refresh sends the right client id/secret per flow
- **End-to-end test:** a new ignored test (`e2e_stream_decrypt`) verifies the full pipeline from session load through decryption to a valid FLAC magic byte check and Symphonia probe

---
