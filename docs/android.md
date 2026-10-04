Android support is an experimental Joystick + Crosshair preview. It requires an
ARM64 device running Android 8 or newer with a compatible Vulkan GPU. The APK
contains the actual client and Go helpers. First launch asks for Minecraft EULA
consent, fetches the pinned official sample resource pack and compiles the required
carriers into private storage. Keep the app open during preparation.

The joystick moves, dragging the world turns, and separate fingers can hold jump,
sneak, sprint, attack or use. The top buttons open pause, chat and inventory.
Inventory and server-form controls use a captured touch pointer. The Android
keyboard feeds the same text editors used on desktop.

Use the test APK attached to the Android workflow on the pull request. For a local
build, install the toolchain pinned in `packaging/android/runtime.json`, set
`ANDROID_HOME` and `ANDROID_NDK_HOME`, then run
`python3 packaging/android/build.py --abi arm64-v8a`. Test APKs use a development
signing key. Builds signed with a different key require uninstalling the old APK;
uninstalling clears its private data.

The other touch schemes, exact native layout/scale/safe-area rules, tap/hold world
targeting, touch inventory gestures, crouch toggles, auto-jump and configurable
controls, touch scrolling, keyboard composition and Android Back are incomplete.
Device frames, audio, suspension/resume and performance
need Android acceptance. See `plan.md` for the open parity gates.
