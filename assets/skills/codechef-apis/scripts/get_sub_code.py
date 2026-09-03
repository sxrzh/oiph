#!/usr/bin/env python3
"""Fetch the source code of a public CodeChef submission."""

import argparse
import sys

from _common import api_get, fail


def main():
    parser = argparse.ArgumentParser(
        description="Get the source code of a public CodeChef submission "
        "(submission id from list_explained_subs.py or a viewsolution URL)."
    )
    parser.add_argument("submission_id", help="submission id, e.g. 92355122")
    args = parser.parse_args()

    data = api_get(
        f"/api/submission-code/{args.submission_id}",
        referer=f"https://www.codechef.com/viewsolution/{args.submission_id}",
    )
    if data.get("status") != "success":
        fail(f"cannot fetch submission {args.submission_id}: "
             f"{data.get('message') or data}")

    code = data.get("data", {}).get("code")
    if code is None:
        fail(f"submission {args.submission_id} has no code (not public or deleted)")
    sys.stdout.write(code.replace("\r", ""))


if __name__ == "__main__":
    main()
