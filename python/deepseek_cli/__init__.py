from importlib.metadata import PackageNotFoundError, version
from pathlib import Path
from typing import Optional
import re

__all__ = ["__version__"]


def _version_from_metadata() -> Optional[str]:
    for dist_name in ("DeepSeek-CLI", "deepseek-cli", "DeepSeek_CLI"):
        try:
            return version(dist_name)
        except PackageNotFoundError:
            continue
    return None


def _version_from_pyproject() -> Optional[str]:
    this_file = Path(__file__).resolve()
    for parent in list(this_file.parents)[:6]:
        candidate = parent / "pyproject.toml"
        if not candidate.exists():
            continue
        try:
            contents = candidate.read_text(encoding="utf-8")
        except OSError:
            continue
        match = re.search(r'(?m)^version\\s*=\\s*"([^"]+)"\\s*$', contents)
        if match:
            return match.group(1)
    return None


__version__ = _version_from_metadata() or _version_from_pyproject() or "0.0.0"
