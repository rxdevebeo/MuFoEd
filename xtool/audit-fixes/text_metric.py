#!/usr/bin/env python3
"""Independent text score. Unicode scalars, LCS, CRLF to LF only.

A zero expected length is an error. This is not the production extractor.
"""

from __future__ import annotations

import sys


def normalize_newlines(text: str) -> str:
    return text.replace("\r\n", "\n")


def lcs_length(want: str, have: str) -> int:
    want_chars = list(normalize_newlines(want))
    have_chars = list(normalize_newlines(have))
    if not want_chars or not have_chars:
        return 0
    previous = [0] * (len(have_chars) + 1)
    current = [0] * (len(have_chars) + 1)
    for left in want_chars:
        for index, right in enumerate(have_chars):
            if left == right:
                current[index + 1] = previous[index] + 1
            else:
                current[index + 1] = max(current[index], previous[index + 1])
        previous, current = current, previous
        for index in range(len(current)):
            current[index] = 0
    return previous[len(have_chars)]


def recall(want: str, have: str) -> float:
    if len(normalize_newlines(want)) == 0:
        raise ZeroDivisionError("zero denominator")
    return lcs_length(want, have) / len(normalize_newlines(want))


def precision(want: str, have: str) -> float:
    if len(normalize_newlines(have)) == 0:
        raise ZeroDivisionError("zero denominator")
    return lcs_length(want, have) / len(normalize_newlines(have))


def flawed_recall(want: str, have: str) -> float:
    """The audit's instrument: the search cursor, with spaces removed."""
    want_chars = [ch for ch in want if not ch.isspace()]
    have_chars = [ch for ch in have if not ch.isspace()]
    if not want_chars:
        return 1.0
    index = 0
    for ch in have_chars:
        while index < len(want_chars) and want_chars[index] != ch:
            index += 1
        if index < len(want_chars):
            index += 1
    return index / len(want_chars)


def main() -> int:
    if flawed_recall("abc", "Z") != 1.0:
        sys.stderr.write("flawed instrument no longer reproduces abc→Z = 1\n")
        return 1
    checks = [
        (recall("abc", "Z"), 0.0),
        (recall("abc", "c"), 1.0 / 3.0),
        (recall("abc", "ab"), 2.0 / 3.0),
        (recall("abc", "ac"), 2.0 / 3.0),
        (recall("abc", "bc"), 2.0 / 3.0),
    ]
    for got, expect in checks:
        if abs(got - expect) > 1e-12:
            sys.stderr.write(f"recall {got} != {expect}\n")
            return 1
    if recall("abc", "cba") >= 1.0:
        sys.stderr.write("abc→cba was treated as complete\n")
        return 1
    if precision("abc", "abcZZ") >= 1.0:
        sys.stderr.write("extra text did not lower precision\n")
        return 1
    if abs(recall("a b", "ab") - 2.0 / 3.0) > 1e-12:
        sys.stderr.write("spaces were deleted before the comparison\n")
        return 1
    if recall("a\r\nb", "a\nb") != 1.0:
        sys.stderr.write("CRLF was not normalized to LF\n")
        return 1
    try:
        recall("", "abc")
    except ZeroDivisionError:
        pass
    else:
        sys.stderr.write("empty want was scored\n")
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
