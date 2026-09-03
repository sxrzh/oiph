#!/usr/bin/env python3
"""Fetch a CodeChef problem's content (statement, samples, constraints, limits, tags)."""

import argparse
import json

from _common import add_format_arg, api_get, emit, fail

# JSON-escaped string literal of the placeholder "example problem statement"
# body CodeChef inserts for problems without a real statement; decoded below.
# If a problem's body equals this string, the body is dropped from the output.
USELESS_BODY_JSON = r"""This is an example problem statement in markdown, and a mini guide on writing statements. Please make sure to remove everything here before publishing your problem.\n\n- Codechef uses markdown for its problem statements. Markdown syntax can be found [here](https:\/\/github.com\/showdownjs\/showdown\/wiki\/Showdown's-Markdown-syntax). Note the `[text](link)` syntax to insert a hyperlink.\n- Codechef also uses $\\LaTeX$ to render mathematical expressions, and you are advised to make liberal use of it to make your statement look good.\n- Text can be made **bold** or *italicized*.\n- **Do not** use HTML tags (p, ul, li, pre, br, ...) in the statement.\n- To insert an image, first upload it to an online hosting service (for an official contest, ask a Codechef admin to do this for you \u2014 this is important) and then use the following syntax: `![alt text](link-to-image)`.\n- If your problem doesn't contain subtasks, ensure that the Subtasks section below is disabled and **all content is deleted from it**.\n\nIf you face any issues, either contact a Codechef admin directly or send us an email at help@codechef.com.\n\nBelow is an example problem statement that uses some of the above-mentioned features.\n\n---------\n\nChef has a simple undirected graph $G$ with $N$ vertices and $M$ edges. A [subgraph](https:\/\/mathworld.wolfram.com\/Subgraph.html) $H$ of $G$ is called *good* if:\n- $H$ is connected\n- $H$ contains all $N$ vertices of $G$\n- There is a unique path between any two vertices in $H$, using only edges in $H$\n\nCount the number of *good* subgraphs of $G$. Since this number might be large, report it modulo $10^9 + 7$.\n\nIn other news, here's a completely unrelated image:\n\n![](https:\/\/s3.amazonaws.com\/codechef_shared\/download\/Images\/START41\/ss3.png).\n\n\n<aside style='background: #f8f8f8;padding: 10px 15px;'><div>All submissions for this problem are available.<\/div><\/aside>"""

USELESS_BODY = json.loads('"' + USELESS_BODY_JSON + '"')


def build_output(prob):
    out = {
        "problem_code": prob.get("problem_code"),
        "problem_name": prob.get("problem_name"),
    }
    body = prob.get("body")
    if body is not None and body != USELESS_BODY:
        out["body"] = body
    out["problemComponents"] = prob.get("problemComponents")
    timelimit = prob.get("max_timelimit")
    if timelimit is not None:
        try:
            timelimit = int(float(timelimit) * 1000)
        except (TypeError, ValueError):
            pass
    out["max_timelimit"] = timelimit
    out["difficulty_rating"] = prob.get("difficulty_rating")
    for key in ("best_tag", "user_tags", "computed_tags"):
        value = prob.get(key)
        if value:
            out[key] = value
    return out


def main():
    parser = argparse.ArgumentParser(
        description="Get the content of a CodeChef problem by its problem code "
        "(e.g. TRIANGLE7): statement, samples, constraints, time limit, difficulty, tags."
    )
    parser.add_argument("problem_code", help="problem code, e.g. TRIANGLE7")
    add_format_arg(parser)
    args = parser.parse_args()

    prob = api_get(
        f"/api/contests/PRACTICE/problems/{args.problem_code}",
        referer=f"https://www.codechef.com/problems/{args.problem_code}",
    )
    if prob.get("status") != "success":
        fail(f"cannot fetch problem {args.problem_code}: "
             f"{prob.get('message') or prob.get('error') or prob}")

    emit(build_output(prob), args.format)


if __name__ == "__main__":
    main()
