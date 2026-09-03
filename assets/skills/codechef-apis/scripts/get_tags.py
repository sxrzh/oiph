#!/usr/bin/env python3
"""Fetch all CodeChef problem tags and drop tags with zero problems."""

import argparse

from _common import add_format_arg, api_get, emit, fail


def main():
    parser = argparse.ArgumentParser(
        description="Get all CodeChef problem tags (tags with problemCount 0 are removed). "
        "Use tagSlug (not tagName) when querying problems."
    )
    add_format_arg(parser)
    args = parser.parse_args()

    data = api_get(
        "/api/problems/tags",
        params={"start_rating": 0, "end_rating": 5000},
        referer="https://www.codechef.com/practice-old/tags/algorithms",
    )
    if data.get("status") != "success":
        fail(f"API returned status={data.get('status')!r}: {data.get('message', '')}")

    tags = [
        t
        for t in data.get("data", [])
        if str(t.get("problemCount", "0")).strip() not in ("", "0")
    ]
    emit(tags, args.format)


if __name__ == "__main__":
    main()
