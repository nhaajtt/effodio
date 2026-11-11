"""Runs a list of searches through `magpie-helper serve` and reports how often the expected file comes first.

    python scripts/eval.py evals/testbed.json [path-to-magpie-helper]

The index and settings are the ones MAGPIE_HOME points at.
"""
import json
import subprocess
import os
import sys


def main():
    cases = json.load(open(sys.argv[1], encoding="utf-8"))
    helper = os.path.abspath(sys.argv[2] if len(sys.argv) > 2 else "target/release/magpie-helper.exe")
    process = subprocess.Popen([helper, "serve"], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                               stderr=subprocess.DEVNULL, encoding="utf-8")
    assert json.loads(process.stdout.readline())["event"] == "ready"
    first = top5 = 0
    slowest = 0.0
    for number, case in enumerate(cases):
        request = {"id": number, "op": "search", "q": case["q"], "limit": 10}
        process.stdin.write(json.dumps(request) + "\n")
        process.stdin.flush()
        answer = json.loads(process.stdout.readline())
        if not answer["ok"]:
            print("ERROR", case["q"], answer["error"])
            continue
        result = answer["result"]
        names = [hit["path"].replace("\\", "/").rsplit("/", 1)[1] for hit in result["hits"]]
        expected = case.get("expect")
        if number > 0:
            slowest = max(slowest, result["ms"])
        if expected is None:
            ok = not [hit for hit in result["hits"] if hit["strong"]]
            mark = "ok " if ok else "BAD"
            first += ok
            top5 += ok
        else:
            rank = names.index(expected) + 1 if expected in names else 0
            first += rank == 1
            top5 += 1 <= rank <= 5
            mark = "ok " if rank == 1 else ("top5" if 1 <= rank <= 5 else "MISS")
            where = case.get("loc")
            if rank == 1 and where is not None:
                hit = result["hits"][0]
                if hit["loc"] is None or abs(hit["loc"] - where) > case.get("within", 0):
                    mark = "LOC"
                    first -= 1
        print(f"{mark:5} {result['ms']:6.0f} ms  {case['q']!r} -> {names[:3]}"
              + (f" loc={result['hits'][0]['loc']}" if result["hits"] and case.get("loc") is not None else ""))
    process.stdin.close()
    print(f"\nfirst: {first}/{len(cases)}   top 5: {top5}/{len(cases)}   slowest warm search: {slowest:.0f} ms")


main()
