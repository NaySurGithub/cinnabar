# Experimental Android client

This builds the real Rust/Bevy client as an Android NativeActivity and the Go core/local-world
server as immutable PIE executables inside the APK's native-library directory. The app uses
private internal storage for settings, logs, authentication and generated carriers. It does not
request broad storage access.

The bootstrap Activity asks for the Minecraft EULA, downloads the manifest-pinned official
sample pack and OFL fonts, verifies their hashes, and runs the same checked asset compiler and
preparation plan in-process. It then launches NativeActivity so Bevy receives its first window
events. Mojang-derived packs and carriers never enter the APK or git. The JSON-UI engine draws
the game UI; Android dialogs are used only for preparation and errors.

Install the SDK/API/build-tools/NDK versions in `runtime.json`, the Rust toolchain from
`rust-toolchain.toml`, a JDK, `rsvg-convert` and the manifest's `cargo-ndk` version. Set
`ANDROID_HOME` and `ANDROID_NDK_HOME`, then run:

```sh
python3 packaging/android/build.py --abi arm64-v8a
```

On the shared Fedora host, the command must run through `/home/danick/.local/bin/agent-check`;
heavy local release builds are deferred to GitHub Actions. The `Android APK (experimental)`
workflow uploads a signed installable ARM64 APK. A manual x86_64 build supports emulator checks.
The default key is a local test key, not a production signing identity; a different CI build may
require uninstalling the earlier test APK before installing it. Generated keys/artifacts live
under ignored `.local/` and `target/` directories.

For release signing, pass `--keystore` and `--key-alias`, with
`CINNABAR_ANDROID_STORE_PASSWORD` and `CINNABAR_ANDROID_KEY_PASSWORD` in the environment. APK
verification checks signatures, 16 KB native alignment and the required Rust/Go payload. Android
support is experimental; native touch feel, lifecycle and device performance gates remain open
until tested on an identified Bedrock reference and a physical device.
