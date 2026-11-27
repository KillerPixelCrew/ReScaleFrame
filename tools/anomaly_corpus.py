"""The reviewed local Anomaly shader corpus: refusal, hashing and reading, shared by the transform tools."""
import hashlib
import json
from pathlib import Path


class Refusal(ValueError):
    """The exact reviewed source, operation or safe output boundary did not match."""


def digest(data):
    return hashlib.sha256(data).hexdigest()


def tree(root):
    """Every file under `root` by relative path, refusing any link."""
    result = {}
    for path in sorted(root.rglob('*')):
        if path.is_symlink() or path.is_junction():
            raise Refusal('linked path: ' + str(path))
        if path.is_file():
            result[path.relative_to(root).as_posix()] = path.read_bytes()
    return result


def corpus_hash(files):
    return digest(json.dumps([[k, digest(v)] for k, v in sorted(files.items())],
                             separators=(',', ':'), ensure_ascii=True).encode())