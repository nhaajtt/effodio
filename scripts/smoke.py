"""Asks a running index everything the app asks of it, and says what came back.

    python scripts/smoke.py [path-to-magpie-helper]

The index and settings are the ones MAGPIE_HOME points at. Exits with 1 when an answer is missing or wrong.
"""
import json
import os
import subprocess
import sys


class Helper:
    def __init__(self, path):
        self.process = subprocess.Popen([path, "serve"], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                        stderr=subprocess.DEVNULL, encoding="utf-8")
        assert json.loads(self.process.stdout.readline())["event"] == "ready"
        self.number = 0

    def ask(self, **request):
        self.number += 1
        request["id"] = self.number
        self.process.stdin.write(json.dumps(request) + "\n")
        self.process.stdin.flush()
        answer = json.loads(self.process.stdout.readline())
        if not answer["ok"]:
            raise RuntimeError(f"{request['op']}: {answer['error']}")
        return answer["result"]


def name(path):
    return path.replace("\\", "/").rsplit("/", 1)[1]


def main():
    helper = Helper(os.path.abspath(sys.argv[1] if len(sys.argv) > 1 else "target/release/magpie-helper.exe"))
    failures = []

    def check(label, ok, detail=""):
        print(("ok   " if ok else "FAIL ") + label + (f"  {detail}" if detail else ""))
        if not ok:
            failures.append(label)

    counts = helper.ask(op="counts")["counts"]
    check("counts", counts["files"] > 0, f"{counts['files']} files, {counts['vectors']} vectors")

    found = helper.ask(op="search", q="a dog on the beach", limit=5)
    hits = found["hits"]
    check("search", bool(hits), ", ".join(name(hit["path"]) for hit in hits[:3]))
    first = hits[0]

    like = helper.ask(op="similar", file=first["file"], limit=5)["hits"]
    check("more like this", bool(like) and all(hit["file"] != first["file"] for hit in like),
          ", ".join(name(hit["path"]) for hit in like[:3]))

    by_example = helper.ask(op="example", path=first["path"], limit=5)["hits"]
    check("search by picture", bool(by_example), ", ".join(name(hit["path"]) for hit in by_example[:3]))

    typing = helper.ask(op="search", q="payment decl", typing=True, limit=3)["hits"]
    check("search while typing", bool(typing) and "Screenshot" in typing[0]["path"],
          typing[0]["excerpt"] if typing else "")

    filtered = helper.ask(op="search", q="kind:video", limit=10)["hits"]
    check("filters alone", bool(filtered) and all(hit["kind"] == "video" for hit in filtered), f"{len(filtered)} videos")

    video = next((hit for hit in helper.ask(op="search", q="zebra kind:video", limit=3)["hits"]), None)
    if video:
        moments = helper.ask(op="moments", file=video["file"], q="zebra")["moments"]
        best = max(moments, key=lambda moment: moment["z"]) if moments else None
        check("moments", bool(best) and best["loc"] < 9, f"{len(moments)} moments, best at {best['loc'] if best else None} s")
        frame = helper.ask(op="frame", path=video["path"], second=10, side=320)
        check("video frame", os.path.getsize(frame["path"]) > 1000, name(frame["path"]))
    else:
        check("moments", False, "no video in the index")

    pdf = next((hit for hit in helper.ask(op="search", q="clause about pets kind:pdf", limit=3)["hits"]), None)
    if pdf:
        page = helper.ask(op="page", path=pdf["path"], page=int(pdf["loc"] or 1), side=800)
        check("pdf page", os.path.getsize(page["path"]) > 1000 and pdf["loc"] == 3, f"page {pdf['loc']}, {page['width']}x{page['height']}")
        text = helper.ask(op="text", segment=pdf["segment"])["text"]
        check("segment text", bool(text) and "PETS" in text, (text or "")[:60])
    else:
        check("pdf page", False, "no PDF found")

    groups = helper.ask(op="duplicates")["groups"]
    check("duplicates", True, f"{len(groups)} groups" + (": " + ", ".join(name(member["path"]) for member in groups[0]) if groups else ""))

    points = helper.ask(op="map", limit=500)["points"]
    check("map", len(points) >= 3 and all(abs(point["x"]) < 1e6 for point in points), f"{len(points)} points")

    added = helper.ask(op="collection_add", name="Beach", q="a dog on the beach")["id"]
    listed = helper.ask(op="collections")["collections"]
    helper.ask(op="collection_remove", collection=added)
    after = helper.ask(op="collections")["collections"]
    check("collections", any(c["id"] == added for c in listed) and all(c["id"] != added for c in after))

    nothing = helper.ask(op="search", q="quantum chromodynamics lattice", limit=5)["hits"]
    check("an honest nothing", not [hit for hit in nothing if hit["strong"]], f"{len(nothing)} weak")

    helper.process.stdin.close()
    print(f"\n{len(failures)} failed" if failures else "\nall good")
    sys.exit(1 if failures else 0)


main()
