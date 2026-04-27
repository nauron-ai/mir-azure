#!/usr/bin/env python3
import glob
import os
import sys
from fnmatch import fnmatch


def parse_args(argv: list[str]) -> tuple[int, list[str], list[str]]:
    args = argv[1:]
    limit = 250
    if args and not args[0].startswith("--"):
        limit = int(args[0])
        args = args[1:]

    exts: list[str] = []
    excludes: list[str] = []
    for arg in args:
        if arg.startswith("--exclude="):
            excludes.append(arg.split("=", 1)[1])
        else:
            exts.append(arg.lstrip(".").lower())

    return limit, exts or ["rs"], excludes


def should_skip(path: str) -> bool:
    skip_dirs = {
        "node_modules",
        "target",
        "dist",
        "build",
        ".git",
        ".venv",
        ".mypy_cache",
        ".ruff_cache",
        "__pycache__",
        ".svelte-kit",
        "coverage",
    }
    return bool(skip_dirs & set(path.split(os.sep)))


def is_excluded(path: str, excludes: list[str]) -> bool:
    rel_path = os.path.relpath(path)
    return any(fnmatch(rel_path, pattern) for pattern in excludes)


def list_files(exts: list[str], excludes: list[str]) -> list[str]:
    files: set[str] = set()
    for ext in exts:
        for candidate in glob.glob(f"**/*.{ext}", recursive=True):
            if os.path.isdir(candidate) or should_skip(candidate) or is_excluded(candidate, excludes):
                continue
            files.add(candidate)
    return sorted(files)


def count_loc(path: str) -> int:
    with open(path, "r", encoding="utf-8", errors="ignore") as handle:
        return len(handle.read().splitlines())


def main() -> None:
    limit, exts, excludes = parse_args(sys.argv)
    violations = [(path, count_loc(path)) for path in list_files(exts, excludes)]
    violations = [(path, loc) for path, loc in violations if loc > limit]
    if violations:
        print(f"Files exceeding LOC limit (limit={limit})")
        for path, loc in sorted(violations, key=lambda item: (-item[1], item[0])):
            print(f"  {path}: {loc}")
        sys.exit(1)
    print(f"LOC check passed for {', '.join(exts)} (limit={limit}).")


if __name__ == "__main__":
    main()
