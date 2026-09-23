#!/usr/bin/env python3
"""Snap 24 reference oracle.

An independent, deliberately-naive solution enumerator used as the correctness
reference for the Rust solver. It uses a different algorithm on purpose so that
a bug in one implementation is unlikely to be mirrored by the other:

    for every binary tree shape over n leaves
      for every permutation of the dealt cards onto those leaves
        for every assignment of + - * / to the internal nodes
          evaluate with exact rationals (fractions.Fraction)
          if it equals the target, canonicalize and de-duplicate

Solutions are canonicalized per ``docs/canonical-form.md`` (canonical form
version ``v1``) and compared as canonical serializations, so commutative /
associative ``+`` and ``*`` collapse to one solution while ``-`` and ``/`` stay
ordered and binary.

Regenerate the golden fixtures:

    python3 crates/snap24-core/tests/reference/oracle.py \
        --write crates/snap24-core/tests/fixtures.json

Self-check (no files written):

    python3 crates/snap24-core/tests/reference/oracle.py --self-check
"""

import argparse
import json
import sys
from fractions import Fraction
from itertools import permutations, product

CANONICAL_FORM = "v1"
OPS = "+-*/"
LEAF = ("leaf",)


# --------------------------------------------------------------------------- #
# tree shapes                                                                  #
# --------------------------------------------------------------------------- #

def shapes(n):
    """All binary tree shapes with n leaves. Count == Catalan(n - 1)."""
    if n == 1:
        return [LEAF]
    out = []
    for left in range(1, n):
        for l in shapes(left):
            for r in shapes(n - left):
                out.append(("node", l, r))
    return out


def evaluate(shape, vals, ops):
    """Evaluate ``shape`` with leaves from ``vals`` and internal ops from ``ops``.

    Returns a Fraction, or None if any division by zero happens.
    """
    vi = 0
    oi = 0

    def go(s):
        nonlocal vi, oi
        if s is LEAF:
            v = vals[vi]
            vi += 1
            return v
        op = ops[oi]
        oi += 1
        a = go(s[1])
        if a is None:
            return None
        b = go(s[2])
        if b is None:
            return None
        if op == "+":
            return a + b
        if op == "-":
            return a - b
        if op == "*":
            return a * b
        if b == 0:
            return None
        return a / b

    return go(shape)


def build(shape, vals, ops):
    vi = 0
    oi = 0

    def go(s):
        nonlocal vi, oi
        if s is LEAF:
            v = vals[vi]
            vi += 1
            return ("leaf", v)
        op = ops[oi]
        oi += 1
        return ("op", op, go(s[1]), go(s[2]))

    return go(shape)


# --------------------------------------------------------------------------- #
# canonical form (docs/canonical-form.md, v1)                                  #
# --------------------------------------------------------------------------- #

def canonicalize(node):
    if node[0] == "leaf":
        return node
    op, l, r = node[1], canonicalize(node[2]), canonicalize(node[3])
    if op in ("+", "*"):
        kids = []
        for c in (l, r):
            if c[0] == "n" and c[1] == op:
                kids.extend(c[2])
            else:
                kids.append(c)
        return ("n", op, kids)
    return ("b", op, l, r)


def serialize(node):
    if node[0] == "leaf":
        return str(node[1])
    if node[0] == "n":
        kids = sorted((serialize(c) for c in node[2]), key=lambda s: s.encode("utf-8"))
        return "(" + node[1] + "," + ",".join(kids) + ")"
    return "(" + node[1] + "," + serialize(node[2]) + "," + serialize(node[3]) + ")"


def rational_str(value):
    return str(value) if value.denominator == 1 else f"{value.numerator}/{value.denominator}"


# --------------------------------------------------------------------------- #
# enumeration                                                                  #
# --------------------------------------------------------------------------- #

def enumerate_hand(cards):
    """Return {value: set(canonical serialization)} over every raw tree."""
    n = len(cards)
    tree_shapes = shapes(n)
    by_value = {}
    for perm in set(permutations(cards)):
        vals = [Fraction(x) for x in perm]
        for shape in tree_shapes:
            for ops in product(OPS, repeat=n - 1):
                value = evaluate(shape, vals, ops)
                if value is None:
                    continue
                by_value.setdefault(value, set()).add(serialize(canonicalize(build(shape, vals, ops))))
    return by_value


def solve(cards, target):
    by_value = enumerate_hand(cards)
    return by_value.get(Fraction(target), set())


# --------------------------------------------------------------------------- #
# fixtures                                                                     #
# --------------------------------------------------------------------------- #

CASES = [
    ([6, 4, 1], "24"),            # 3 cards, target 24
    ([3, 3, 8, 8], "24"),         # 4 cards, duplicate ranks
    ([1, 1, 3, 4], "7/2"),        # 4 cards, custom rational target
    ([1, 1, 1, 1, 8], "24"),      # 5 cards, spec worked example
    ([9, 10, 9, 9, 1], "24"),     # 5 cards, known unsolvable
    ([2, 2, 2, 2, 2, 2], "64"),   # 6 cards, custom target, duplicate ranks
    ([4, 4, 4, 4, 4, 4], "24"),   # 6 cards, target 24
]


def build_fixtures():
    by_hand = {}
    order = []
    for cards, _ in CASES:
        key = tuple(cards)
        if key not in by_hand:
            order.append(key)
            by_hand[key] = enumerate_hand(cards)

    hands = []
    for key in order:
        by_value = by_hand[key]
        cases = [
            {"target": target, "solutions": sorted(by_value.get(Fraction(target), set()))}
            for cards, target in CASES
            if tuple(cards) == key
        ]
        hands.append(
            {
                "cards": list(key),
                "reachable": sorted((rational_str(v) for v in by_value), key=sort_key),
                "cases": cases,
            }
        )
    return {
        "canonical_form": CANONICAL_FORM,
        "oracle": "crates/snap24-core/tests/reference/oracle.py",
        "generated_by": (
            "python3 crates/snap24-core/tests/reference/oracle.py "
            "--write crates/snap24-core/tests/fixtures.json"
        ),
        "hands": hands,
    }


def sort_key(value_str):
    if "/" in value_str:
        num, den = value_str.split("/")
        return (Fraction(int(num), int(den)),)
    return (Fraction(int(value_str)),)


# --------------------------------------------------------------------------- #
# self-check                                                                   #
# --------------------------------------------------------------------------- #

def self_check():
    spec = solve([1, 1, 1, 1, 8], 24)
    assert len(spec) == 10, f"expected 10 canonical solutions, got {len(spec)}"
    assert solve([9, 10, 9, 9, 1], 24) == set(), "expected [9,10,9,9,1] -> 24 to be unsolvable"
    assert solve([3, 3, 8, 8], 24) == {"(/,8,(-,3,(/,8,3)))"}, solve([3, 3, 8, 8], 24)
    assert solve([1, 1, 2, 2], Fraction(7, 3)), "expected a rational-target solution"
    print("self-check ok: [1,1,1,1,8] -> 24 = 10 canonical solutions")


def main(argv):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--write", metavar="PATH", help="write golden fixtures to PATH")
    parser.add_argument("--self-check", action="store_true", help="run the built-in checks and exit")
    args = parser.parse_args(argv)

    if args.write:
        fixtures = build_fixtures()
        with open(args.write, "w") as fh:
            json.dump(fixtures, fh, indent=1)
            fh.write("\n")
        total = sum(len(c["solutions"]) for h in fixtures["hands"] for c in h["cases"])
        print(f"wrote {args.write}: {len(fixtures['hands'])} hands, {total} solutions")
        self_check()
        return 0

    self_check()
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
