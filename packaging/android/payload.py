"""APK resources contain original/open inputs and registries; Mojang carriers stay per-user."""
from __future__ import annotations

import hashlib
import json
import re
import zipfile
from pathlib import Path


def rust_constant(source: Path, name: str) -> str:
    match = re.search(rf'const {name}: &str = "([^"]+)";', source.read_text())
    if not match:
        raise ValueError(f"missing canonical constant {name} in {source}")
    return match[1]


def file_sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        while block := source.read(1024 * 1024):
            digest.update(block)
    return digest.hexdigest()


def stage_resources(root: Path, destination: Path, runtime: dict) -> Path:
    carriers = root / "crates/assets/src/carriers.rs"
    font_manifest = root / rust_constant(carriers, "FONT_MANIFEST")
    font = json.loads(font_manifest.read_text())
    filename = font["font_file"]
    if Path(filename).name != filename or filename in ("", ".", "..") or "\\" in filename:
        raise ValueError("bundled font manifest has an invalid basename")
    source_font = root / "assets/fonts" / filename
    if source_font.stat().st_size != font["font_size_bytes"] or file_sha256(source_font) != font["font_sha256"].lower():
        raise ValueError("bundled font failed the pinned size/SHA-256 check")
    assets_dir = destination / "apk-assets"
    assets_dir.mkdir(parents=True, exist_ok=True)
    archive = assets_dir / runtime["resource_archive"]
    with zipfile.ZipFile(archive, "w", compression=zipfile.ZIP_DEFLATED, compresslevel=6) as payload:
        payload.writestr("prep-kit/scripts/", "")
        target = json.loads((root / "assets/bedrock-target.json").read_text())
        physics = root / target["artifacts"]["physics_registry"]
        if file_sha256(physics) != target["hashes"]["physics_registry"]:
            raise ValueError("physics registry failed its pinned hash")
        payload.write(physics, f"assets/{physics.name}")
        payload.write(root / "THIRD_PARTY_NOTICES.md", "assets/THIRD_PARTY_NOTICES.md")
        payload.write(source_font, f"fonts/{font['font_file']}")
        payload.write(source_font, f"prep-kit/assets/fonts/{font['font_file']}")
        for source in sorted((root / "assets/licenses").iterdir()):
            if source.is_file():
                payload.write(source, f"licenses/{source.name}")
        for source in sorted((root / "assets").glob("*.json")):
            payload.write(source, f"prep-kit/assets/{source.name}")
        # Registry inputs are already tracked and licensed; never copy a .local carrier/cache.
        for source in sorted((root / "crates/assets/data").iterdir()):
            if source.is_file():
                payload.write(source, f"prep-kit/data/{source.name}")
    return assets_dir
