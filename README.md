# Mini Pi

A desktop GUI chat application for the `pi` coding agent SDK. Built with Rust and GPUI.

## Features

- Native chat threads with model selection
- Workspace / project directory support
- Supabase authentication and agent-config sync
- Optional phone remote control over a Cloudflare Tunnel

## Install

### macOS

Download the latest `mini-pi-x64.dmg` from the [Releases](../../releases) page, open it, and drag **Mini Pi** into your **Applications** folder.

### Windows

Download the latest `mini-pi-x64.msi` from the [Releases](../../releases) page and run it. The installer adds a **Mini Pi** shortcut to the Start Menu.

## Build from source

```bash
# Install bridge dependencies
cd pi-bridge && npm install && npm run build && cd ..

# Run in development
cargo run

# Run tests
cargo test
```

### Build platform installers locally

**Windows** (PowerShell):

```powershell
pwsh -ExecutionPolicy Bypass -File scripts\build-windows.ps1
```

**macOS**:

```bash
./scripts/build-macos.sh
```

Both scripts build a release binary, bundle a private Node.js runtime and the `pi-bridge` SDK, and produce an installer in `target/`.

## Development prerequisites

- Rust stable >= 1.92
- Node.js >= 20 (for the SDK bridge)
- Windows: WiX v3.11.2 is downloaded automatically by the build script
- macOS: `cargo-bundle` and `create-dmg` are installed automatically by the build script
