#!/usr/bin/env python3
"""Search/list CodeChef problems by difficulty range, tags, name keyword, sorted by difficulty."""

import argparse

from _common import add_format_arg, api_get, emit, fail


def normalize_tags(raw):
    if not raw:
        return ""
    parts = []
    for t in raw.split(","):
        t = "-".join(t.lower().split())
        if t:
            parts.append(t)
    return ",".join(parts)


def main():
    parser = argparse.ArgumentParser(
        description="List CodeChef problems filtered by difficulty range, tags and "
        "name/code keyword, sorted by difficulty. Tags must be tagSlug values from "
        "get_tags.py (comma separated)."
    )
    parser.add_argument("--probs-per-page", type=int, default=20,
                        help="max problems per page (API limit), default: 20")
    parser.add_argument("--page-index", type=int, default=0,
                        help="page number starting from 0 (API page), default: 0")
    parser.add_argument("--sort-order", choices=["asc", "desc"], default="asc",
                        help="sort order by difficulty_rating, default: asc")
    parser.add_argument("--search", default="",
                        help="keyword matched against problem name and problem code")
    parser.add_argument("--start-rating", type=int, default=0,
                        help="minimum difficulty, default: 0")
    parser.add_argument("--end-rating", type=int, default=5001,
                        help="maximum difficulty, default: 5001 (-1 means unrated)")
    parser.add_argument("--tags", default="",
                        help="comma-separated tagSlugs")
    parser.add_argument("--include-unrated", action="store_true",
                        help="keep problems with difficulty_rating=-1 (unrated); "
                        "they are filtered out by default")
    add_format_arg(parser)
    args = parser.parse_args()

    tags = normalize_tags(args.tags)
    data = api_get(
        "/api/list/problems",
        params={
            "page": args.page_index,
            "limit": args.probs_per_page,
            "sort_by": "difficulty_rating",
            "sort_order": args.sort_order,
            "search": args.search,
            "category": "rated",
            "start_rating": args.start_rating,
            "end_rating": args.end_rating,
            "topic": "",
            "tags": tags,
            "group": "all",
        },
        referer="https://www.codechef.com/practice-old",
    )
    if data.get("status") != "success":
        fail(f"API returned status={data.get('status')!r}: {data.get('message', '')}")

    problems = data.get("data", [])
    if not args.include_unrated:
        problems = [p for p in problems if str(p.get("difficulty_rating")) != "-1"]
    problems = [
        {k: v for k, v in p.items()
         if k not in ("id", "intended_contest_id", "actual_intended_contests",
                      "contest_code")}
        for p in problems
    ]
    emit(problems, args.format)


if __name__ == "__main__":
    main()
