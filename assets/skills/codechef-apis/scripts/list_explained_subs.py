#!/usr/bin/env python3
"""List a CodeChef problem's public annotated (explained) submissions."""

import argparse

from _common import add_format_arg, api_get, emit, fail


def main():
    parser = argparse.ArgumentParser(
        description="List the public annotated submissions of a CodeChef problem "
        "(only accepted submissions with result_code=1)."
    )
    parser.add_argument("problem_code", help="problem code, e.g. TRIANGLE7")
    add_format_arg(parser)
    args = parser.parse_args()

    data = api_get(
        "/api/annotations/top",
        params={"problemCode": args.problem_code},
        referer=f"https://www.codechef.com/problems/{args.problem_code}",
    )
    if data.get("status") != "success":
        fail(f"cannot fetch annotations for {args.problem_code}: "
             f"{data.get('message') or data}")

    keep = [
        {"submission_id": a["submission_id"], "language": a.get("language")}
        for a in data.get("annotations", [])
        if str(a.get("result_code")) == "1"
    ]
    emit(keep, args.format)


if __name__ == "__main__":
    main()
