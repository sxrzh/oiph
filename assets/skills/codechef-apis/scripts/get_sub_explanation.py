#!/usr/bin/env python3
"""Fetch the author's line-by-line explanation of a public CodeChef submission."""

import argparse

from _common import add_format_arg, api_get, emit, fail


def main():
    parser = argparse.ArgumentParser(
        description="Get the author-written code explanation of a public CodeChef "
        "submission (line ranges + annotation text)."
    )
    parser.add_argument("submission_id", help="submission id, e.g. 92355122")
    add_format_arg(parser)
    args = parser.parse_args()

    data = api_get(
        "/api/annotations",
        params={"submission_id": args.submission_id},
        referer=f"https://www.codechef.com/viewsolution/{args.submission_id}",
    )
    if data.get("status") != "success":
        fail(f"cannot fetch explanation of submission {args.submission_id}: "
             f"{data.get('message') or data}")

    keep = [
        {
            "from_line": a.get("from_line"),
            "to_line": a.get("to_line"),
            "annotation": a.get("annotation"),
        }
        for a in data.get("annotations", [])
    ]
    emit(keep, args.format)


if __name__ == "__main__":
    main()
