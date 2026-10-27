# This page is the BONELAB Lemonifier source code
# [CLICK HERE FOR THE MODDING APP](TODO)

The **BONELAB Lemonifier** is a fork of ModsBeforeFriday for **BONELAB on Quest** that works entirely
within the browser, using WebUSB to interact with the headset. The aim is to make installing mods as easy as possible,
with no need to download special tools or sideload an installer APK.

## How it works

BONELAB is modded using [LemonLoader](https://github.com/LemonLoader/MelonLoader), a fork of
[MelonLoader](https://github.com/LavaGang/MelonLoader) with support for standalone Android/Quest games.

Instead of the old (Beat Saber) approach of installing mods via a separate modloader folder and QMOD packages,
the Lemonifier patches the BONELAB APK itself in the same way as the
[official LemonLoader installer](https://github.com/LemonLoader/MelonLoaderInstaller):

Mods are then plain `.dll` files, managed in `/sdcard/MelonLoader/com.StressLevelZero.BONELAB/Mods`
(and `.../Mods/disabled` for mods that are switched off). Mods are installed from
[Thunderstore](https://thunderstore.io/c/bonelab/), via drag 'n' drop, file upload or URL import.

## Query Parameters

- `?game_id=<package>`: Mods a different app with the given package ID. (Defaults to `com.StressLevelZero.BONELAB`.)
- `?ignore_package_id=true`: Skips the game ID check during mod import.

## Project Structure

- `./mbf-agent` contains the agent, which is an executable written in Rust that is executed by the frontend via ADB. This agent does pretty much all the work, including downloading LemonLoader, patching the game and managing mods.
- `./mbf-agent-wrapper` is a Python script that can be used to invoke the MBF backend with a command-line-interface, handy for developers or Chromium-haters.
- `./mbf-adb-killer` is a development utility that kills any running ADB server when the frontend tries to connect to your Quest, thus avoiding conflicts between MBF and other apps *during development only.*.
- `./mbf-res-man` contains the MBF resource management project, which contains code used by MBF to access external resources e.g. LemonLoader releases and Unity dependencies.
- `./mbf-zip` is a simple library for reading/writing ZIP files (and signing APKs) used by the `mbf-agent`.
- `./mbf-axml` is a library for reading/writing Android binary XML (used to patch the game's manifest).
- `./mbf-site` contains the frontend, which communicates with the agent via JSON. (Written in typescript with React).
- `./ya-webadb` is a git submodule containing the (forked) WebADB libraries used to talk to the Quest from the browser.

## Compilation Instructions
### Build Requirements
- [pnpm](https://pnpm.io/installation) (the CI uses pnpm 9)
- Rust 1.77 or newer, install with [rustup](https://rustup.rs/).
- [Android NDK](https://developer.android.com/ndk/downloads), r23b or newer.

### Setting up Rust
Install the aarch64-linux-android target:

```$ rustup target add aarch64-linux-android```

#### Environment Variables
- Set `ANDROID_NDK_HOME` to the folder containing your Android NDK.
- Set `CC_aarch64-linux-android` to `$NDK_PATH/toolchains/llvm/prebuilt/windows-x86_64/bin/aarch64-linux-android31-clang.cmd` where `$NDK_PATH` is your Android NDK root path.
- Set `AR_aarch64-linux-android` to `$NDK_PATH/toolchains/llvm/prebuilt/windows-x86_64/bin/llvm-ar.exe`.

(if on another OS, the paths may be slightly different. Please update the paths as necessary!)
#### Cargo config
Create a new file with path `~/.cargo/config.toml`. Add the following contents, replacing the `<contents of...>` with the relevant environment variable.
```toml
[target.aarch64-linux-android]
linker = "<contents OF CC_aarch64-linux-android environment variable>"
ar = "<contents OF AR_aarch64-linux-android environment variable>"
```

### Compiling Agent
- To compile the agent and copy it to the `public` directory so that it can be used by the site, run `./build_agent.ps1`.

### Debugging site
To serve the site for testing, navigate to `./mbf-site` and run `pnpm start`.
(you may need to `pnpm install` at the repo root first, so that the workspace packages are available).

## Credits
- [Lauriethefish](https://github.com/Lauriethefish) and co. for the original ModsBeforeFriday project that this is based on.