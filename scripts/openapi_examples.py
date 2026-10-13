"""Reject credential-bearing upload URLs in published OpenAPI examples."""
from urllib.parse import parse_qs, urlsplit

SIGNED_URL_PARAMETERS = frozenset({
    "x-amz-credential", "x-amz-security-token", "x-amz-signature",
})


def signed_url_paths(value, path="$"):
    if isinstance(value, dict):
        for key, child in value.items():
            pointer_key = str(key).replace("~", "~0").replace("/", "~1")
            yield from signed_url_paths(child, f"{path}/{pointer_key}")
    elif isinstance(value, list):
        for index, child in enumerate(value):
            yield from signed_url_paths(child, f"{path}/{index}")
    elif isinstance(value, str) and value.startswith(("https://", "http://")):
        parameters = {key.lower() for key in parse_qs(urlsplit(value).query)}
        if parameters & SIGNED_URL_PARAMETERS:
            yield path


def assert_safe_examples(value):
    paths = list(signed_url_paths(value))
    if paths:
        raise ValueError(
            "Replace signed upload examples with https://uploads.example.com/audio.mp3: "
            + ", ".join(paths)
        )
