# SYNK Music Discord RPC
This is a simple tool that connects SYNK Music to your Discord profile, so your friends can see what you're listening to in real-time.
## Status
WIP - honestly it's pretty new, so there are probably some bugs. If it breaks, it breaks. Make a issue on here if you feel like it's a serious issue, or make a PR if you think you can fix it.
## How do I get the SYNK Music RPC?
At the moment we have 2 ways of getting the SYNK Music RPC.
## 1: Build it from source (recommended)
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
$script = Invoke-RestMethod -Uri "https://encore.synkteam.uk/build.ps1"
Invoke-Expression "$script"
```
Then just open SYNK Music in your browser, and it should connect automatically.

## 2: Download the pre-compiled version from the releases.
Head over to the [Releases](../../releases) page and grab the latest `.exe`. No Rust required, just download it, run it, and you're good to go.

## Usage
1. Make sure Discord is open and running.
2. Open [SYNK Music](https://synkmusic.com) in your browser and start playing something.
3. Run the RPC tool (however you installed it).
4. That's it, your Discord status should update automatically.

### Custom port
If port 19836 is already in use, you can change it:
```bash
synkmusic-rpc -p 19837
```
If you change the port here, you'll also need to change the default websocket port on the website. Go to **Profile Picture > Settings > Privacy and Social > RPC Websocket Port** and set it to the same port.

## FAQ

### Is this a virus?
No. If you don't believe us then you can check the source code for yourselves, it's all right here. You can also build it yourself on your own computer if you want to be 100% sure. See the [build from source](#1-build-it-from-source-recommended) section above.

### Why isn't my status showing on Discord?
Make sure you've got Activity Status enabled in Discord. Go to **Settings > Activity Privacy** and turn on **"Share your activity status"**. Also make sure Discord is actually running before you start the RPC, it can't update your status if there's nothing to connect to.

## Contributing
PRs are welcome. The codebase is pretty small so it shouldn't be too hard to find your way around. If you're planning something big, open an issue first so we can talk about it before you sink time into it.

## License
MIT, do whatever you want with it.
