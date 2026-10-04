#!/usr/bin/env python3
"""Build a signed experimental APK with the real Rust client and immutable Go helpers."""
from __future__ import annotations

import argparse
import json
import os
import re
import shutil
import subprocess
import tomllib
import zipfile
from pathlib import Path
from xml.sax.saxutils import escape

from payload import stage_resources

ROOT = Path(__file__).resolve().parents[2]
RUNTIME = json.loads((ROOT / "packaging/android/runtime.json").read_text())
ANDROID_CLIENT = ROOT / "packaging/android/client"
CLIENT_PACKAGE = tomllib.loads((ANDROID_CLIENT / "Cargo.toml").read_text())["package"]["name"]
CLIENT_LIBRARY = CLIENT_PACKAGE.replace("-", "_")
ABIS = {
    "arm64-v8a": ("aarch64-linux-android", "aarch64-linux-android", "arm64", "AArch64"),
    "x86_64": ("x86_64-linux-android", "x86_64-linux-android", "amd64", "Advanced Micro Devices X86-64"),
}


def execute(command: list[str | Path], *, cwd: Path = ROOT, env: dict | None = None) -> None:
    subprocess.run([str(value) for value in command], cwd=cwd, env=env, check=True)


def product_name() -> str:
    source = (ROOT / "crates/launcher/src/lib.rs").read_text()
    match = re.search(r'macro_rules! product_name\s*\{\s*\(\)\s*=>\s*\{\s*"([^"]+)"', source)
    if not match:
        raise ValueError("cannot read the launcher's canonical product name")
    return match[1]


def render(template: Path, destination: Path, values: dict) -> None:
    text = template.read_text()
    for name, value in values.items():
        text = text.replace(f"@{name}@", str(value))
    if re.search(r"@[A-Z_]+@", text):
        raise ValueError(f"unresolved template token in {template}")
    destination.parent.mkdir(parents=True, exist_ok=True)
    destination.write_text(text)


def verify_elf(path: Path, readelf: Path, machine: str) -> None:
    headers = subprocess.check_output([str(readelf), "--wide", "--file-header", "--program-headers", str(path)], text=True)
    if machine not in headers or not re.search(r"Type:\s+DYN", headers):
        raise ValueError(f"{path} is not the expected Android shared library/PIE")
    loads = [line.split() for line in headers.splitlines() if line.strip().startswith("LOAD ")]
    if not loads or any(int(line[-1], 16) < 16384 for line in loads):
        raise ValueError(f"{path} is missing Android 16 KB LOAD alignment")


def build_native(args: argparse.Namespace, ndk: Path, output: Path, triple: str, clang_triple: str, goarch: str) -> dict[str, Path]:
    toolchain = ndk / "toolchains/llvm/prebuilt/linux-x86_64"
    compiler = toolchain / "bin" / f"{clang_triple}{RUNTIME['min_sdk']}-clang"
    if not compiler.is_file():
        raise FileNotFoundError(compiler)
    target_root = Path(os.environ.get("CARGO_TARGET_DIR", ROOT / "target")).resolve()
    rust_profile = "release" if args.profile == "release" else args.profile
    client = target_root / triple / rust_profile / f"lib{CLIENT_LIBRARY}.so"
    natives = {client.name: client}
    natives.update({name: output / name for name in (RUNTIME["core_library"], RUNTIME["local_server_library"])})
    if not args.skip_native_build:
        environment = os.environ.copy()
        environment.update(ANDROID_NDK_HOME=str(ndk), CARGO_BUILD_JOBS="2", GOMAXPROCS="2")
        environment["RUSTFLAGS"] = environment.get("RUSTFLAGS", "") + " -C link-arg=-Wl,-z,max-page-size=16384"
        execute(["cargo", "ndk", "-t", args.abi, "-p", str(RUNTIME["min_sdk"]), "build", "--locked", "-p", CLIENT_PACKAGE, "--profile", args.profile], env=environment)
        environment.update(GOOS="android", GOARCH=goarch, CGO_ENABLED="1", CC=str(compiler), GOWORK="off", GOFLAGS=environment.get("GOFLAGS", "-p=1"))
        # anet's Android interface workaround requires private net linknames on Go 1.23+.
        flags = "-s -w -checklinkname=0 -extldflags=-Wl,-z,max-page-size=16384"
        execute(["go", "build", "-buildmode=pie", "-trimpath", "-ldflags", flags, "-o", natives[RUNTIME["core_library"]], "./cmd/bedrock-core"], cwd=ROOT / "core", env=environment)
        execute(["go", "build", "-buildmode=pie", "-trimpath", "-ldflags", flags, "-o", natives[RUNTIME["local_server_library"]], "."], cwd=ROOT / "tools/localserver", env=environment)
    libcxx = toolchain / "sysroot/usr/lib" / clang_triple / "libc++_shared.so"
    natives[libcxx.name] = libcxx
    for file in natives.values():
        if not file.is_file():
            raise FileNotFoundError(f"missing Android native payload: {file}")
    readelf = toolchain / "bin/llvm-readelf"
    for file in natives.values():
        verify_elf(file, readelf, ABIS[args.abi][3])
    symbols = subprocess.check_output([str(readelf), "--dyn-symbols", "--wide", str(client)], text=True)
    for symbol in ("ANativeActivity_onCreate", "JNI_OnLoad"):
        if not re.search(rf"\b{symbol}\b", symbols):
            raise ValueError(f"Rust Android library lacks required entry point {symbol}")
    return natives


def build_apk(args: argparse.Namespace) -> Path:
    sdk_root = os.environ.get("ANDROID_HOME") or os.environ.get("ANDROID_SDK_ROOT")
    if not sdk_root:
        raise ValueError("set ANDROID_HOME to the isolated Android SDK")
    sdk = Path(sdk_root)
    ndk = Path(os.environ.get("ANDROID_NDK_HOME", sdk / "ndk" / RUNTIME["ndk"]))
    tools = sdk / "build-tools" / RUNTIME["build_tools"]
    android_jar = sdk / "platforms" / f"android-{RUNTIME['target_sdk']}" / "android.jar"
    output = ROOT / "target/android" / args.abi
    output.mkdir(parents=True, exist_ok=True)
    triple, clang_triple, goarch, _ = ABIS[args.abi]
    natives = build_native(args, ndk, output, triple, clang_triple, goarch)
    assets_dir = stage_resources(ROOT, output, next(file for name, file in natives.items() if name == f"lib{CLIENT_LIBRARY}.so"), RUNTIME)
    version = tomllib.loads((ROOT / "Cargo.toml").read_text())["workspace"]["package"]["version"]
    major, minor, patch = (int(value) for value in version.split("."))
    version_code = major * 1_000_000 + minor * 1_000 + patch
    values = {"APPLICATION_ID": RUNTIME["application_id"], "ACTIVITY": RUNTIME["activity"],
              "CLIENT_LIBRARY": CLIENT_LIBRARY, "PRODUCT_NAME": product_name(),
              "ARCHIVE_BYTES": RUNTIME["archive_limits"]["archive_bytes"]}
    source_root = output / "java"
    classes = output / "classes"
    dex = output / "dex"
    for directory in (source_root, classes, dex):
        shutil.rmtree(directory, ignore_errors=True)
        directory.mkdir(parents=True)
    for name in ("BootstrapActivity", RUNTIME["activity"]):
        render(ROOT / "packaging/android" / f"{name}.java", source_root / f"{name}.java", values)
    execute(["javac", "--release", "8", "-classpath", android_jar, "-d", classes, *sorted(source_root.glob("*.java"))])
    execute([tools / "d8", "--min-api", str(RUNTIME["min_sdk"]), "--lib", android_jar, "--output", dex, *sorted(classes.rglob("*.class"))])
    manifest = output / "AndroidManifest.xml"
    manifest.write_text(f'''<manifest xmlns:android="http://schemas.android.com/apk/res/android"
    package="{RUNTIME['application_id']}" android:versionName="{version}" android:versionCode="{version_code}">
    <uses-sdk android:minSdkVersion="{RUNTIME['min_sdk']}" android:targetSdkVersion="{RUNTIME['target_sdk']}" />
    <uses-permission android:name="android.permission.INTERNET" />
    <uses-permission android:name="android.permission.ACCESS_NETWORK_STATE" />
    <uses-feature android:name="android.hardware.vulkan.level" android:version="1" android:required="false" />
    <application android:label="{escape(product_name())}" android:icon="@drawable/icon"
        android:theme="@android:style/Theme.Material.NoActionBar" android:hasCode="true"
        android:allowBackup="false" android:usesCleartextTraffic="false" android:extractNativeLibs="true"
        android:debuggable="{str(not args.keystore).lower()}">
        <activity android:name=".BootstrapActivity" android:exported="true" android:screenOrientation="landscape"
            android:configChanges="orientation|keyboardHidden|screenSize|screenLayout|uiMode|density">
            <intent-filter><action android:name="android.intent.action.MAIN" /><category android:name="android.intent.category.LAUNCHER" /></intent-filter>
        </activity>
        <activity android:name=".{RUNTIME['activity']}" android:exported="false" android:screenOrientation="landscape"
            android:launchMode="singleTask"
            android:configChanges="orientation|keyboardHidden|screenSize|screenLayout|uiMode|density">
            <meta-data android:name="android.app.lib_name" android:value="{CLIENT_LIBRARY}" />
        </activity>
    </application>
</manifest>\n''')
    resources = output / "res/drawable"
    resources.mkdir(parents=True, exist_ok=True)
    execute(["rsvg-convert", "--width", "192", "--height", "192", "--output", resources / "icon.png", ROOT / "packaging/icons/cinnabar.svg"])
    unsigned = output / "unsigned.apk"
    execute([tools / "aapt", "package", "-f", "-M", manifest, "-S", resources.parent, "-A", assets_dir, "-I", android_jar, "-F", unsigned])
    with zipfile.ZipFile(unsigned, "a", compression=zipfile.ZIP_STORED) as apk:
        apk.write(dex / "classes.dex", "classes.dex")
        for name, file in natives.items():
            apk.write(file, f"lib/{args.abi}/{name}")
    aligned = output / "aligned.apk"
    execute([tools / "zipalign", "-P", "16", "-f", "4", unsigned, aligned])
    keystore = Path(args.keystore) if args.keystore else ROOT / ".local/android/test.keystore"
    alias = args.key_alias or "android-test"
    if not args.keystore and not keystore.exists():
        keystore.parent.mkdir(parents=True, exist_ok=True)
        execute(["keytool", "-genkeypair", "-keystore", keystore, "-storepass", "android", "-keypass", "android", "-alias", alias,
                 "-keyalg", "RSA", "-keysize", "2048", "-validity", "3650", "-dname", "CN=Android Test Build"])
    apk = output / f"{product_name()}-{args.abi}-test.apk"
    environment = os.environ.copy()
    if not args.keystore:
        environment["CINNABAR_ANDROID_STORE_PASSWORD"] = "android"
        environment["CINNABAR_ANDROID_KEY_PASSWORD"] = "android"
    execute([tools / "apksigner", "sign", "--ks", keystore, "--ks-key-alias", alias,
             "--ks-pass", "env:CINNABAR_ANDROID_STORE_PASSWORD", "--key-pass", "env:CINNABAR_ANDROID_KEY_PASSWORD",
             "--min-sdk-version", str(RUNTIME["min_sdk"]), "--out", apk, aligned], env=environment)
    execute([tools / "apksigner", "verify", "--verbose", apk])
    execute([tools / "zipalign", "-c", "-P", "16", "4", apk])
    verify_apk(apk, args.abi, natives)
    print(apk)
    return apk


def verify_apk(path: Path, abi: str, natives: dict[str, Path]) -> None:
    with zipfile.ZipFile(path) as apk:
        expected = {"AndroidManifest.xml", "classes.dex", f"assets/{RUNTIME['resource_archive']}"}
        expected.update(f"lib/{abi}/{name}" for name in natives)
        if missing := expected - set(apk.namelist()):
            raise ValueError(f"APK lacks required payload: {sorted(missing)}")
        if bad := apk.testzip():
            raise ValueError(f"APK contains a corrupt entry: {bad}")
        if any(name.endswith((".mcbea", ".mcbeatm", ".mcbeent", ".mcbehud", ".mcbeui")) for name in apk.namelist()):
            raise ValueError("APK must never ship Mojang-derived carriers")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--abi", choices=ABIS, default="arm64-v8a")
    parser.add_argument("--profile", choices=("release", "play"), default="release")
    parser.add_argument("--skip-native-build", action="store_true", help="assemble already-built native payloads")
    parser.add_argument("--keystore", help="optional release signing keystore")
    parser.add_argument("--key-alias", help="alias in the optional release keystore")
    build_apk(parser.parse_args())
