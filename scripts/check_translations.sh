#!/usr/bin/env bash
# Check the desktop app's French and English catalogues against the source.
#
#   scripts/check_translations.sh           fail on a stale template or a missing,
#                                           fuzzy or malformed translation
#   scripts/check_translations.sh --update  re-extract lang/messages.pot and merge it
#                                           into both catalogues, then fill in English
#
# Needs GNU gettext 0.24 or later (Rust support in xgettext).
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root/crates/yoto-desktop"

if ! xgettext --help 2>/dev/null | grep -q 'Rust'; then
    echo 'xgettext with Rust support (GNU gettext 0.24 or later) is required.' >&2
    exit 1
fi

update=false
case "${1:-}" in
    '') ;;
    --update) update=true ;;
    *) echo "usage: $0 [--update]" >&2; exit 2 ;;
esac

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

# Strings are French in the source: tr!("…"), trn!("…", "…", n), trc!("context", "…").
# File-only locations keep the template stable when code moves within a file.
find src -name '*.rs' | LC_ALL=C sort > "$tmp/files"
xgettext --language=Rust --from-code=UTF-8 --keyword \
    --keyword='tr!:1' --keyword='trn!:1,2' --keyword='trc!:1c,2' \
    --add-comments=TRANSLATORS: --add-location=file \
    --package-name=yoto-desktop \
    --msgid-bugs-address=https://github.com/damione1/yoto-media-manager/issues \
    --files-from="$tmp/files" --output="$tmp/messages.pot"

# The creation date changes on every run and is not part of the template.
strip_date() { grep -v '^"POT-Creation-Date:' "$1"; }

if $update; then
    cp "$tmp/messages.pot" lang/messages.pot
    for lang in en fr; do
        msgmerge --quiet --update --backup=none --no-fuzzy-matching \
            --add-location=file "lang/$lang/LC_MESSAGES/yoto-desktop.po" lang/messages.pot
        msgattrib --no-obsolete --add-location=file \
            -o "lang/$lang/LC_MESSAGES/yoto-desktop.po" "lang/$lang/LC_MESSAGES/yoto-desktop.po"
    done
    # French is the source language: its catalogue repeats the ids.
    msgen --add-location=file -o lang/fr/LC_MESSAGES/yoto-desktop.po lang/fr/LC_MESSAGES/yoto-desktop.po
    echo 'Catalogues updated; translate the empty English entries, then run the check.'
    exit 0
fi

if ! diff -u <(strip_date lang/messages.pot) <(strip_date "$tmp/messages.pot"); then
    echo 'lang/messages.pot is out of date: run scripts/check_translations.sh --update.' >&2
    exit 1
fi

for lang in en fr; do
    po="lang/$lang/LC_MESSAGES/yoto-desktop.po"
    merged="$tmp/$lang.po"
    # A check must not rewrite catalogues or accept fuzzy guesses when a
    # source string changes.
    msgmerge --quiet --no-fuzzy-matching -o "$merged" "$po" lang/messages.pot
    msgfmt --check-format -o /dev/null "$merged"
    python3 "$root/scripts/check_translations.py" "$lang" "$merged"
done

echo 'Translations check passed.'
