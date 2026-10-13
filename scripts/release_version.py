#!/usr/bin/env python3
"""Reject version mismatches before a tagged native build starts."""
import os, pathlib, re, tomllib
ROOT = pathlib.Path(__file__).resolve().parents[1]
version = tomllib.loads((ROOT / 'Cargo.toml').read_text())['workspace']['package']['version']
tag = os.environ.get('GITHUB_REF_NAME', '')
if not re.fullmatch(r'v(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)', tag) or tag != 'v' + version:
    raise SystemExit('Release tag must be v' + version + ', matching Cargo.toml')
print('version=' + version)
