"""Create a signed development package using the public ALCOMD signing contract."""

import argparse
import hashlib
import json
from pathlib import Path
import struct
import subprocess
import tempfile
import zipfile


def openssl(executable: str, *arguments: str) -> bytes:
    return subprocess.run([executable, *arguments], check=True, stdout=subprocess.PIPE).stdout


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--component", required=True, type=Path)
    parser.add_argument("--key", required=True, type=Path, help="Ed25519 private key PEM")
    parser.add_argument("--out", required=True, type=Path)
    parser.add_argument("--openssl", default="openssl")
    args = parser.parse_args()
    if args.out.suffix != ".alcomdextension":
        parser.error("output must end in .alcomdextension")

    public_der = openssl(args.openssl, "pkey", "-in", str(args.key), "-pubout", "-outform", "DER")
    spki_prefix = bytes.fromhex("302a300506032b6570032100")
    if not public_der.startswith(spki_prefix) or len(public_der) != len(spki_prefix) + 32:
        parser.error("the signing key must be Ed25519")
    public_key = public_der[len(spki_prefix):]
    fingerprint = "ed25519-sha256:" + hashlib.sha256(public_key).hexdigest()

    template = Path(__file__).resolve().parents[1] / "alcomd-extension.toml"
    placeholder = b"ed25519-sha256:" + b"0" * 64
    manifest = template.read_bytes()
    if manifest.count(placeholder) != 1:
        parser.error("the manifest template must contain a single publisher key placeholder")
    component = args.component.read_bytes()
    if not component.startswith(b"\x00asm\x0d\x00\x01\x00") or len(component) > 32 * 1024 * 1024:
        parser.error("a bounded WebAssembly component is required")
    entries = {
        "alcomd-extension.toml": manifest.replace(placeholder, fingerprint.encode("ascii")),
        "component/extension.wasm": component,
    }
    digest = hashlib.sha256(b"ALCOMD-EXT-CONTENT-SHA256-V1\0")
    for path, data in sorted(entries.items()):
        path_bytes = path.encode("utf-8")
        digest.update(struct.pack("<I", len(path_bytes)))
        digest.update(path_bytes)
        digest.update(struct.pack("<Q", len(data)))
        digest.update(data)
    with tempfile.TemporaryDirectory() as directory:
        message = Path(directory) / "message.bin"
        message.write_bytes(b"ALCOMD-EXT-SIGNATURE-V1\0" + digest.digest())
        signature = openssl(args.openssl, "pkeyutl", "-sign", "-rawin", "-inkey", str(args.key), "-in", str(message))
    if len(signature) != 64:
        parser.error("signer returned an invalid Ed25519 signature")
    entries["META-INF/alcomd-signature-v1.json"] = json.dumps({
        "formatVersion": 1,
        "algorithm": "ed25519",
        "packageDigest": digest.hexdigest(),
        "publicKey": public_key.hex(),
        "publisherFingerprint": fingerprint,
        "signature": signature.hex(),
    }, separators=(",", ":")).encode("utf-8")

    with zipfile.ZipFile(args.out, "x", compression=zipfile.ZIP_STORED, allowZip64=False) as archive:
        for path, data in sorted(entries.items()):
            archive.writestr(zipfile.ZipInfo(path), data)
    print(f"{args.out}  {digest.hexdigest()}")


if __name__ == "__main__":
    main()
