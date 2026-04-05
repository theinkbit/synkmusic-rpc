# Encore Discord RPC

This is a simple tool that connects Encore to your Discord profile, so your friends can see what you're listening to in real-time.

## Status
WIP - honestly it's pretty new, so there are probably some bugs. If it breaks, it breaks. Make a issue on here if you feel like it's a serious issue, or make a PR if you think you can fix it.

## How to use it

### 1. Get Rust
You need Rust on your machine to run this. Just run this in your terminal:
```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```
(If you're on Windows, just grab the installer from [rustup.rs](https://rustup.rs/)). 
Restart your terminal after you're done.

### 2. Run it
Download the project, open the folder in your terminal, and run:
```bash
cargo run --release
```

### Or you could use a really easy powershell script
```powershell
Set-ExecutionPolicy -ExecutionPolicy RemoteSigned -Scope Process -Force
$script = Invoke-RestMethod -Uri "https://tomsystems.org/encore.ps1"
Invoke-Expression "$script"```

Then just open Encore in your browser, and it should connect automatically.