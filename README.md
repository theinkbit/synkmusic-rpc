# SynkMusic Discord RPC

Display your SynkMusic listening activity as Discord Rich Presence.

## Installation

### Build from source

Requires [Rust](https://rustup.rs/).

```bash
cargo run --release
```

### Pre-built binary

Download the latest release from the [Releases](../../releases) page — no Rust required.

### Quick install (Windows)

```powershell
Set-ExecutionPolicy -ExecutionPolicy RemoteSigned -Scope Process -Force
$script = Invoke-RestMethod -Uri "https://synkmusic.synkteam.uk/build.ps1"
Invoke-Expression "$script"
```

## Usage

1. Open Discord.
2. Start playing music on [SynkMusic](https://synkmusic.synkteam.uk).
3. Run `synkmusic-rpc`.
4. Your Discord status updates automatically.

## Contributing

PRs welcome. For larger changes, open an issue first.

## License

MIT
