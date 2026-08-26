#!/usr/bin/env python3
# Copyright 2026 VKrishna04
# SPDX-License-Identifier: Apache-2.0

"""Build platform wheels for PyPI out of the release assets.

dev-prune is a Rust program with no Python in it. It is on PyPI anyway because that is
where `uv tool install`, `uvx` and `pipx run` look, and those are the fastest ways for a
lot of people to get a CLI onto a machine. Nothing here makes it a Python project: each
wheel is a zip carrying one prebuilt executable and enough metadata to describe it.

The binaries go in `<name>-<version>.data/scripts/`, which installers unpack straight
into the environment's `bin`/`Scripts` directory. That is the mechanism uv and ruff use
to ship themselves, and it needs no Python shim, no compiler, and no build backend.

Deliberately stdlib-only. This runs in the release workflow between the Rust build and
the upload, and a wheel builder that needs its own `pip install` step is one more thing
that can break a release.

Usage: python scripts/build_wheels.py <version> <assets-dir> <out-dir>
  assets-dir  holds dev-prune-v<version>-<os>-<arch>.{tar.gz,zip} from the build job
  out-dir     receives the .whl files
"""

from __future__ import annotations

import argparse
import base64
import csv
import hashlib
import io
import re
import sys
import tarfile
import zipfile
from pathlib import Path

DIST_NAME = "dev-prune"
# PEP 503/427: the name is normalised for the filename and the .dist-info directory.
WHEEL_NAME = DIST_NAME.replace("-", "_")
SUMMARY = (
    "Universal, lockfile-safe workspace pruner. Reclaims disk space from idle Git "
    "repositories by deleting only dependency and build directories a lockfile can "
    "rebuild."
)
HOMEPAGE = "https://devprune.vkrishna04.me"
REPOSITORY = "https://github.com/Life-Experimentalist/dev-prune"
DOCUMENTATION = f"{REPOSITORY}/blob/main/docs/README.md"
# PyPI indexes these for search, and this is the only place they get set — there is no
# pyproject.toml to read them from.
KEYWORDS = ",".join(
    [
        "cleanup",
        "prune",
        "disk-space",
        "node_modules",
        "monorepo",
        "lockfile",
        # One per language group. A Go or Elixir developer searching PyPI for a
        # cleaner is not going to type "node_modules".
        "npm",
        "pnpm",
        "yarn",
        "bun",
        "venv",
        "uv",
        "poetry",
        "cargo",
        "go",
        "gradle",
        "maven",
        "composer",
        "bundler",
        "cocoapods",
        "swift",
        "elixir",
        "terraform",
        "flutter",
    ]
)

# asset name -> platform tags for the wheel.
#
# The Linux binaries are statically linked against musl, so they carry no libc
# dependency at all. That satisfies the manylinux policy trivially — the policy is a
# ceiling on which shared libraries may be required, and these require none — while also
# being the literal musllinux case. One binary, both tag families, so Alpine and Debian
# users get a wheel from the same asset instead of only one of them being served.
TARGETS = {
    ("linux", "x64"): [
        "manylinux_2_17_x86_64",
        "manylinux2014_x86_64",
        "musllinux_1_2_x86_64",
    ],
    ("linux", "arm64"): [
        "manylinux_2_17_aarch64",
        "manylinux2014_aarch64",
        "musllinux_1_2_aarch64",
    ],
    # The floors match what the Rust targets themselves support: x86_64-apple-darwin
    # runs back to 10.12, and aarch64 Macs did not exist before 11.0.
    ("darwin", "x64"): ["macosx_10_12_x86_64"],
    ("darwin", "arm64"): ["macosx_11_0_arm64"],
    ("windows", "x64"): ["win_amd64"],
    ("windows", "arm64"): ["win_arm64"],
    # `win32` is pip's tag for 32-bit x86 Windows, not for Windows in general — the
    # name predates 64-bit and has never meant what it looks like it means.
    ("windows", "x86"): ["win32"],
}

# Classifiers are PyPI's browse facets, and the two added below are the two true
# things about this tool that none of the others say: it finds its work by walking Git
# repositories, and emptying a shared cache on a build machine is administration
# rather than development.
CLASSIFIERS = [
    "Development Status :: 5 - Production/Stable",
    "Environment :: Console",
    "Intended Audience :: Developers",
    "Intended Audience :: System Administrators",
    "License :: OSI Approved :: Apache Software License",
    "Operating System :: MacOS",
    "Operating System :: Microsoft :: Windows",
    "Operating System :: POSIX :: Linux",
    "Programming Language :: Rust",
    "Topic :: Software Development :: Build Tools",
    "Topic :: Software Development :: Version Control :: Git",
    "Topic :: System :: Filesystems",
    "Topic :: System :: Systems Administration",
    "Topic :: Utilities",
]


def absolutize_readme(readme: str, version: str) -> str:
    """Point every relative link and image in the README at GitHub.

    crates.io and npmjs.com resolve relative README links against the `repository`
    field. PyPI does not — it renders the description as-is, so `assets/hero_banner.png`
    is a broken image and `docs/CLI_REFERENCE.md` is a 404 on the project page. Rewriting
    here rather than in README.md keeps the file readable in the repository, where the
    relative form is the correct one.

    Pinned to the tag, not to `main`: the page for 1.0.0 should show 1.0.0's docs.
    """
    raw = f"{REPOSITORY.replace('github.com', 'raw.githubusercontent.com')}/v{version}"
    blob = f"{REPOSITORY}/blob/v{version}"
    absolute = re.compile(r"^(?:[a-z][a-z0-9+.-]*:|//|#)")

    def rewrite(target: str, base: str) -> str:
        return target if absolute.match(target) else f"{base}/{target.lstrip('./')}"

    # Images first, so the second pass sees them as already absolute and leaves them
    # alone. That is what makes `[![License](shield)](LICENSE.md)` come out right: the
    # inner image is handled here and the outer link target by the pass below.
    readme = re.sub(
        r"(!\[[^\]]*\]\()([^)\s]+)(\))",
        lambda m: m[1] + rewrite(m[2], raw) + m[3],
        readme,
    )
    readme = re.sub(
        r"(\]\()([^)\s]+)(\))",
        lambda m: m[1] + rewrite(m[2], blob) + m[3],
        readme,
    )
    return re.sub(
        r'(<img\s[^>]*?\bsrc=")([^"]+)(")',
        lambda m: m[1] + rewrite(m[2], raw) + m[3],
        readme,
    )


def urlsafe_b64_nopad(data: bytes) -> str:
    """The digest encoding PEP 427 specifies for RECORD: urlsafe base64, no padding."""
    return base64.urlsafe_b64encode(data).decode("ascii").rstrip("=")


def read_binary_from_asset(assets: Path, version: str, os_name: str, arch: str) -> bytes:
    """Pull the single executable out of the release archive for one platform."""
    if os_name == "windows":
        archive = assets / f"{DIST_NAME}-v{version}-{os_name}-{arch}.zip"
        member = "dev-prune.exe"
        if not archive.is_file():
            raise SystemExit(f"missing asset: {archive}")
        with zipfile.ZipFile(archive) as zf:
            try:
                return zf.read(member)
            except KeyError:
                raise SystemExit(f"{archive} does not contain {member}") from None

    archive = assets / f"{DIST_NAME}-v{version}-{os_name}-{arch}.tar.gz"
    member = "dev-prune"
    if not archive.is_file():
        raise SystemExit(f"missing asset: {archive}")
    with tarfile.open(archive, "r:gz") as tf:
        extracted = tf.extractfile(member)
        if extracted is None:
            raise SystemExit(f"{archive} does not contain {member}")
        return extracted.read()


def metadata(version: str, readme: str) -> str:
    lines = [
        "Metadata-Version: 2.1",
        f"Name: {DIST_NAME}",
        f"Version: {version}",
        f"Summary: {SUMMARY}",
        "Author: VKrishna04",
        "License: Apache-2.0",
        f"Keywords: {KEYWORDS}",
        f"Project-URL: Homepage, {HOMEPAGE}",
        f"Project-URL: Documentation, {DOCUMENTATION}",
        f"Project-URL: Source, {REPOSITORY}",
        f"Project-URL: Issues, {REPOSITORY}/issues",
        f"Project-URL: Changelog, {REPOSITORY}/blob/main/CHANGELOG.md",
    ]
    lines += [f"Classifier: {c}" for c in CLASSIFIERS]
    lines += [
        # No floor worth stating beyond "a Python that understands modern wheel tags".
        # Nothing in the wheel is executed by Python.
        "Requires-Python: >=3.8",
        "Description-Content-Type: text/markdown",
        "",
        readme,
    ]
    return "\n".join(lines)


def wheel_file(tags: list[str]) -> str:
    lines = [
        "Wheel-Version: 1.0",
        "Generator: dev-prune build_wheels.py",
        # False: the wheel is platform-specific and its contents are not pure Python.
        "Root-Is-Purelib: false",
    ]
    lines += [f"Tag: py3-none-{tag}" for tag in tags]
    return "\n".join(lines) + "\n"


def build_wheel(
    out_dir: Path,
    version: str,
    tags: list[str],
    binary: bytes,
    exe_suffix: str,
    readme: str,
    license_text: str,
) -> Path:
    dist_info = f"{WHEEL_NAME}-{version}.dist-info"
    data_scripts = f"{WHEEL_NAME}-{version}.data/scripts"

    entries: list[tuple[str, bytes, int]] = []

    # Both names, because both are real entry points. `devp` is not a shell alias in any
    # other install path — it is a second executable — and an install that produced only
    # `dev-prune` would quietly make every `devp` in the docs wrong.
    for name in ("dev-prune", "devp"):
        entries.append((f"{data_scripts}/{name}{exe_suffix}", binary, 0o755))

    entries.append((f"{dist_info}/METADATA", metadata(version, readme).encode(), 0o644))
    entries.append((f"{dist_info}/WHEEL", wheel_file(tags).encode(), 0o644))
    entries.append(
        (f"{dist_info}/licenses/LICENSE.md", license_text.encode(), 0o644)
    )

    record_rows = [
        (path, f"sha256={urlsafe_b64_nopad(hashlib.sha256(blob).digest())}", len(blob))
        for path, blob, _ in entries
    ]
    # RECORD cannot hash itself; PEP 427 says its own row carries empty fields.
    record_rows.append((f"{dist_info}/RECORD", "", ""))
    buf = io.StringIO()
    csv.writer(buf, lineterminator="\n").writerows(record_rows)
    entries.append((f"{dist_info}/RECORD", buf.getvalue().encode(), 0o644))

    filename = f"{WHEEL_NAME}-{version}-py3-none-{'.'.join(tags)}.whl"
    path = out_dir / filename
    with zipfile.ZipFile(path, "w", zipfile.ZIP_DEFLATED) as zf:
        for name, blob, mode in entries:
            # A fixed timestamp keeps the wheel byte-identical across rebuilds of the
            # same assets, so a re-run of the release produces the same artifact.
            info = zipfile.ZipInfo(name, date_time=(1980, 1, 1, 0, 0, 0))
            # 0o100000 is S_IFREG. Without the file-type bits some extractors read the
            # entry as having no type at all and drop the permission bits with it, which
            # would land the binaries non-executable.
            info.external_attr = (0o100000 | mode) << 16
            info.compress_type = zipfile.ZIP_DEFLATED
            zf.writestr(info, blob)
    return path


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("version")
    parser.add_argument("assets", type=Path)
    parser.add_argument("out", type=Path)
    args = parser.parse_args()

    repo_root = Path(__file__).resolve().parent.parent
    readme = absolutize_readme(
        (repo_root / "README.md").read_text(encoding="utf-8"), args.version
    )
    license_text = (repo_root / "LICENSE.md").read_text(encoding="utf-8")

    args.out.mkdir(parents=True, exist_ok=True)

    for (os_name, arch), tags in TARGETS.items():
        binary = read_binary_from_asset(args.assets, args.version, os_name, arch)
        path = build_wheel(
            out_dir=args.out,
            version=args.version,
            tags=tags,
            binary=binary,
            exe_suffix=".exe" if os_name == "windows" else "",
            readme=readme,
            license_text=license_text,
        )
        print(f"built {path.name} ({path.stat().st_size / 1024:.0f} KiB)")

    return 0


if __name__ == "__main__":
    sys.exit(main())
