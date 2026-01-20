"""Thin wrapper that downloads and runs the DeepSeek CLI binary."""

import os
import platform
import stat
import sys
from pathlib import Path
from urllib.request import urlopen

from deepseek_cli import __version__

REPO = "Hmbown/DeepSeek-CLI"


def main() -> None:
    """Entry point - resolve binary and exec it."""
    binary = resolve_binary()
    os.execv(binary, [binary, *sys.argv[1:]])


def resolve_binary() -> str:
    """Find or download the deepseek binary."""
    # Allow override via environment
    override = os.getenv("DEEPSEEK_CLI_PATH")
    if override and Path(override).exists():
        return override

    # Check cache
    cache_dir = Path.home() / ".deepseek" / "bin" / __version__
    cache_dir.mkdir(parents=True, exist_ok=True)

    asset_name = get_asset_name()
    bin_name = "deepseek.exe" if sys.platform == "win32" else "deepseek"
    dest = cache_dir / bin_name

    if dest.exists():
        return str(dest)

    if os.getenv("DEEPSEEK_CLI_SKIP_DOWNLOAD") in ("1", "true", "TRUE"):
        raise RuntimeError("deepseek binary not found and downloads are disabled.")

    # Download from GitHub releases
    url = f"https://github.com/{REPO}/releases/download/v{__version__}/{asset_name}"
    print(f"Downloading DeepSeek CLI v{__version__}...", file=sys.stderr)
    download_binary(url, dest)
    return str(dest)


def get_asset_name() -> str:
    """Get the release asset name for this platform."""
    system = platform.system().lower()
    arch = platform.machine().lower()

    if system == "linux" and arch in ("x86_64", "amd64"):
        return "deepseek-linux-x64"
    if system == "darwin" and arch in ("arm64", "aarch64"):
        return "deepseek-macos-arm64"
    if system == "darwin" and arch in ("x86_64", "amd64"):
        return "deepseek-macos-x64"
    if system == "windows" and arch in ("x86_64", "amd64", "amd64"):
        return "deepseek-windows-x64.exe"

    raise RuntimeError(f"Unsupported platform: {system}/{arch}")


def download_binary(url: str, dest: Path) -> None:
    """Download binary from URL to destination."""
    try:
        with urlopen(url, timeout=60) as response:
            data = response.read()
    except Exception as e:
        raise RuntimeError(f"Failed to download: {e}") from e

    dest.write_bytes(data)

    # Make executable on Unix
    if sys.platform != "win32":
        dest.chmod(dest.stat().st_mode | stat.S_IXUSR | stat.S_IXGRP | stat.S_IXOTH)

    print(f"Installed to {dest}", file=sys.stderr)


if __name__ == "__main__":
    main()
