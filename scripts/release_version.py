#!/usr/bin/env python3
"""Turn the pushed release tag (vX.Y.Z) into the version every build step receives."""
import os, sys
from distribution_utils import is_release_version
tag = os.environ.get('GITHUB_REF_NAME', '')
if not tag.startswith('v') or not is_release_version(tag[1:]):
    sys.exit('Release tag must look like v1.2.3, got ' + repr(tag))
print('version=' + tag[1:])
