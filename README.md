# ALCOMD Discord Presence

This ALCOMD extension shows an approved Unity editing session in Discord desktop. It uses the same public Discord Application ID as ALCOMD v3 and defaults to showing “Editing in Unity”. Project name, Unity version, editor count, and custom text are opt-in settings.

The extension uses the public `background-integration-v1` WIT. The frozen WIT snapshot is in `backend/wit`. The Core handles project observations, local peer authentication, and permission revocation. The guest handles Discord framing, presence and reconnection, and renders settings through Portable UI.

## Build a development package

Use Rust 1.97.1, Python 3, and OpenSSL 3 with Ed25519 support. Run these commands from an independent checkout.

```sh
rustup target add wasm32-unknown-unknown
cargo test --locked --manifest-path backend/Cargo.toml --target-dir target/backend
cargo build --locked --release --target wasm32-unknown-unknown --manifest-path backend/Cargo.toml --target-dir target/backend
cargo run --locked --manifest-path tools/component/Cargo.toml --target-dir target/tools -- target/backend/wasm32-unknown-unknown/release/alcomd_extension_discord_backend.wasm target/extension.wasm
openssl genpkey -algorithm ED25519 -out target/dev-key.pem
python tools/package.py --component target/extension.wasm --key target/dev-key.pem --out target/ALCOMD-Discord-Presence.alcomdextension
```

Install the local package through **Extensions** in an ALCOMD build supporting M9. Open the extension details and review the required permissions and local target. Select **Enable** once. Open the extension to review the latest connection status and preview or change privacy settings. Saving settings updates the preview. Closing the ALCOMD window leaves the extension running. Select **Disable** to clear the activity and stop the background guest.

Windows and macOS verify the declared Discord signing identity. On Linux the manifest targets the system package's `/usr/share/discord/Discord` executable. The development signing key belongs in `target` and can be reused to test upgrades of the same package identity.

The source is licensed under AGPL-3.0-only. The vendored WIT files are from the [ALCOMD public extension contract](https://github.com/ALCOMD/ALCOMD/tree/main/specs/extensions/wit/extension-v1).
