"""Puts every model and tool file into the site's R2 bucket, which https://effodio.nhaajt.com/models/<name> serves.

    effodio-helper pack <folder> --all          (fetches and checks every file)
    python scripts/mirror-models.py <folder>

Needs R2 S3 credentials for the bucket, in the environment: R2_ACCOUNT_ID, R2_ACCESS_KEY_ID, R2_SECRET_ACCESS_KEY
(and R2_SESSION_TOKEN for temporary ones). Uses nothing but Python. Files already there at the right size are skipped.
The app checks every file's SHA-256 itself, whatever serves it, so a bad upload is refused, not used.
"""

import datetime
import hashlib
import hmac
import http.client
import os
import sys
import urllib.parse

BUCKET = "effodio-models"


def sign(key, text):
    return hmac.new(key, text.encode(), hashlib.sha256).digest()


def request(method, name, size=0, body=None):
    account = os.environ["R2_ACCOUNT_ID"]
    host = f"{account}.r2.cloudflarestorage.com"
    path = f"/{BUCKET}/{urllib.parse.quote(name)}"
    now = datetime.datetime.now(datetime.UTC)
    stamp, day = now.strftime("%Y%m%dT%H%M%SZ"), now.strftime("%Y%m%d")
    headers = {"host": host, "x-amz-content-sha256": "UNSIGNED-PAYLOAD", "x-amz-date": stamp}
    if os.environ.get("R2_SESSION_TOKEN"):
        headers["x-amz-security-token"] = os.environ["R2_SESSION_TOKEN"]
    if method == "PUT":
        headers["content-length"] = str(size)
        headers["content-type"] = "application/octet-stream"
    names = sorted(headers)
    canonical = "\n".join(
        [method, path, "", "".join(f"{key}:{headers[key]}\n" for key in names), ";".join(names), "UNSIGNED-PAYLOAD"]
    )
    scope = f"{day}/auto/s3/aws4_request"
    to_sign = "\n".join(["AWS4-HMAC-SHA256", stamp, scope, hashlib.sha256(canonical.encode()).hexdigest()])
    key = sign(sign(sign(sign(("AWS4" + os.environ["R2_SECRET_ACCESS_KEY"]).encode(), day), "auto"), "s3"), "aws4_request")
    signature = hmac.new(key, to_sign.encode(), hashlib.sha256).hexdigest()
    headers["authorization"] = (
        f"AWS4-HMAC-SHA256 Credential={os.environ['R2_ACCESS_KEY_ID']}/{scope}, "
        f"SignedHeaders={';'.join(names)}, Signature={signature}"
    )
    connection = http.client.HTTPSConnection(host, timeout=120)
    connection.request(method, path, body=body, headers=headers)
    response = connection.getresponse()
    data = response.read()
    connection.close()
    return response.status, response.getheader("content-length"), data


def main():
    if len(sys.argv) != 2:
        sys.exit(__doc__)
    folder = sys.argv[1]
    names = sorted(name for name in os.listdir(folder) if not name.endswith(".part"))
    for name in names:
        path = os.path.join(folder, name)
        size = os.path.getsize(path)
        status, length, _ = request("HEAD", name)
        if status == 200 and length == str(size):
            print(f"there    {name}")
            continue
        with open(path, "rb") as body:
            status, _, data = request("PUT", name, size, body)
        if status != 200:
            sys.exit(f"{name}: {status} {data[:300].decode(errors='replace')}")
        print(f"put      {name}  {size / 2**20:,.0f} MB")


if __name__ == "__main__":
    main()
