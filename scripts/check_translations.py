#!/usr/bin/env python3
"""Reject missing, fuzzy, copied and placeholder-mismatched catalogue entries."""

import ast
import re
import sys
from pathlib import Path


# Words, names and symbols spelled the same in both languages. An unchanged French
# sentence in the English catalogue is not a translation; make any new exception an
# explicit review decision.
UNCHANGED_ENGLISH = {
    "Audio", "Image", "Image {number}", "Imports", "Original", "Original · {duration}", "Pause",
    "Description", "Playlists", "Podcasts", "Radio", "Streaming", "Volume",
    "{count} playlist", "{count} playlists", "Version {version} ({origin})",
    # Units and numbers only.
    "{hours} h {minutes} min", "{minutes} min {seconds} s", "{seconds} s",
}

PLACEHOLDER = re.compile(r"(?<!\{)\{([A-Za-z_][A-Za-z0-9_]*)\}")


def placeholders(text: str) -> list[str]:
    return sorted(PLACEHOLDER.findall(text))


def check_catalogue(path: Path, language: str) -> list[str]:
    errors: list[str] = []
    entry: dict[str, str] = {}
    flags: set[str] = set()
    current_field = ""

    def finish() -> None:
        msgid = entry.get("msgid")
        if not msgid:
            return
        label = f'{entry.get("msgctxt", "")}: {msgid}'
        if "fuzzy" in flags:
            errors.append(f"fuzzy: {label}")
        plural = entry.get("msgid_plural")
        forms = sorted(key for key in entry if key.startswith("msgstr"))
        translations = [entry[key] for key in forms]
        if not translations or any(not t.strip() for t in translations):
            errors.append(f"untranslated: {label}")
            return
        for translation in translations:
            if re.search(r"\[(?:EN|FR|TODO)\]", translation, re.IGNORECASE):
                errors.append(f"placeholder translation: {label}")
        if language == "fr":
            expected = [msgid] if plural is None else [msgid, plural]
            if translations != expected:
                errors.append(f"French wording differs from the source: {label}")
        if language == "en":
            for source, translation in zip([msgid, plural or msgid], translations):
                if source == translation and translation not in UNCHANGED_ENGLISH:
                    errors.append(f"French text copied into English catalogue: {label}")
        for index, translation in enumerate(translations):
            expected = placeholders(msgid if index == 0 or plural is None else plural)
            found = placeholders(translation)
            if plural is not None:
                # A singular form may say its count in words.
                expected = [name for name in expected if name != "count"]
                found = [name for name in found if name != "count"]
            if expected != found:
                errors.append(f"placeholders differ ({found} vs {expected}): {label}")

    for line in path.read_text(encoding="utf-8").splitlines() + [""]:
        if not line:
            finish()
            entry = {}
            flags = set()
            current_field = ""
        elif line.startswith("#~"):
            continue
        elif line.startswith("#,"):
            flags.update(part.strip() for part in line[2:].split(","))
        elif line.startswith("#"):
            continue
        elif re.match(r"(msgctxt|msgid|msgid_plural|msgstr(\[\d+\])?) ", line):
            current_field, literal = line.split(" ", 1)
            entry[current_field] = ast.literal_eval(literal)
        elif line.startswith('"') and current_field:
            entry[current_field] += ast.literal_eval(line)

    return errors


if __name__ == "__main__":
    language, filename = sys.argv[1:]
    failures = check_catalogue(Path(filename), language)
    if failures:
        for failure in failures:
            print(f"{language}: {failure}", file=sys.stderr)
        sys.exit(1)
    print(f"{language}: all extracted messages translated")
