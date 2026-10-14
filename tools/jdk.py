"""Finds the Java 8 JDK the harness and the remapper run on.

MCRS_JDK8 wins when set. Otherwise JAVA_HOME if it is a Java 8, then the newest jdk-8 / jdk1.8
folder under the usual install roots (Eclipse Adoptium, Java, Zulu, Microsoft).
"""
from __future__ import annotations

import os
import sys
from pathlib import Path


def _is_jdk8(p: Path) -> bool:
    release = p / "release"
    has_javac = (p / "bin" / "javac").is_file() or (p / "bin" / "javac.exe").is_file()
    return has_javac and release.is_file() and 'JAVA_VERSION="1.8' in release.read_text(errors="replace")


def find_jdk8() -> Path:
    if os.environ.get("MCRS_JDK8"):
        return Path(os.environ["MCRS_JDK8"])
    home = os.environ.get("JAVA_HOME")
    if home and _is_jdk8(Path(home)):
        return Path(home)
    roots = [Path(os.environ.get(v, "")) for v in ("ProgramFiles", "ProgramW6432") if os.environ.get(v)]
    roots += [Path("/usr/lib/jvm"), Path("/Library/Java/JavaVirtualMachines")]
    found = []
    for root in roots:
        for vendor in ("Eclipse Adoptium", "Java", "Zulu", "Microsoft", ""):
            base = root / vendor if vendor else root
            if base.is_dir():
                found += [p for p in base.iterdir() if p.is_dir() and _is_jdk8(p / "Contents" / "Home" if (p / "Contents").is_dir() else p)]
    if not found:
        sys.exit("no Java 8 JDK found; install Temurin 8 or set MCRS_JDK8 to its folder")
    best = max(found, key=lambda p: p.name)
    return best / "Contents" / "Home" if (best / "Contents").is_dir() else best
