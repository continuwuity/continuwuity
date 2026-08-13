#!/bin/python3
"""
Nexy's magic changelog linkifier script! Linkify that changelog! Make it clickable!
The masses LOVE being able to click things!

Usage:

    python3 bin/linkify-changelog.py

Assumes CHANGELOG.md is in the current directory.
Will not write if subsequent calls to linkify would change the output (to avoid nested linkification).
"""
import os
import re
import shutil


def linkify(text: str) -> str:
    # ARCANE REGEX RUNES I CALL UPON THEE TO LINKIFY MY SHIT! GO!
    # Linkify issues and PRs
    reformatted = re.sub(
        r"\(#(\d{4})\)",
        r"([#\1](https://forgejo.ellis.link/continuwuation/continuwuity/pulls/\1))",
        text
    )
    # Linkify MSCs
    reformatted = re.sub(
        r"([^\[])(MSC(\d{4}))([^\]])",
        r"\1[\2](https://github.com/matrix-org/matrix-spec-proposals/pull/\3)\4",
        reformatted
    )
    return reformatted


def main():
    with open("CHANGELOG.md") as fd:
        changelog = fd.read()

    linkified = linkify(changelog)
    # verify that double-writes are idempotent
    linkified2 = linkify(linkified)
    assert linkified == linkified2, "Double-linkification was not idempotent!"

    try:
        os.mkdir("out")
    except FileExistsError:
        pass
    shutil.copy("CHANGELOG.md", "out" + os.sep + "CHANGELOG.md.old")
    with open("CHANGELOG.md", "w") as fd:
        fd.write(linkified)


if __name__ == "__main__":
    main()
