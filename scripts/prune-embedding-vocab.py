#!/usr/bin/env python3
"""Cut the bundled Model2Vec model down to the tokens Russian, English and code use.

Input: the full model written by build-embedding-model.py
(src-tauri/models/potion-multilingual-128M-int8). Output: the same model with
every vocabulary entry dropped that contains a letter, digit or mark outside
ASCII and Cyrillic, and the matching rows of the embedding table with it.

Why this loses nothing for Russian, English and code
----------------------------------------------------
The tokenizer is a Unigram model: it segments normalized text into vocabulary
pieces by picking the best path through every piece that *matches* a substring.
A dropped piece contains a character from some other script, so it can never
match text written only in ASCII, Cyrillic, punctuation and symbols. Every such
text therefore has exactly the same candidate pieces, the same best path, the
same token ids (remapped) and the same rows. The parity fixture checks that:
its phrases must come out at cosine 1.0 against the full model's vectors.

What it does lose, deliberately: other scripts (CJK, Arabic, Greek, ...) and
accented Latin (é, ü, ß). Those characters become [UNK], which Model2Vec drops,
so a German or French text is embedded from its unaccented words only. To go
back to the full model, skip this script and point the app at the full build.

Standard library only: the safetensors layout is an 8-byte header length, a
JSON header and raw little-endian bytes, and int8 rows are plain byte slices.
"""

from __future__ import annotations

import argparse
import json
import shutil
import struct
import sys
import unicodedata
from pathlib import Path

SRC_REL = Path("src-tauri/models/potion-multilingual-128M-int8")
OUT_REL = Path("src-tauri/models/potion-multilingual-128M-int8-ru-en")
METASPACE = "▁"


def repo_root() -> Path:
    return Path(__file__).resolve().parents[1]


def keeps(ch: str) -> bool:
    """ASCII, Cyrillic, and any punctuation, symbol or separator."""
    if ch.isascii():
        return True
    if "CYRILLIC" in unicodedata.name(ch, ""):
        return True
    return unicodedata.category(ch)[0] in "PSZ"


def piece_is_kept(piece: str, special: set[str]) -> bool:
    if piece in special:
        return True
    return all(keeps(ch) for ch in piece.replace(METASPACE, ""))


def read_safetensors(path: Path) -> tuple[dict, bytes]:
    raw = path.read_bytes()
    (header_len,) = struct.unpack("<Q", raw[:8])
    header = json.loads(raw[8 : 8 + header_len])
    return header, raw[8 + header_len :]


def write_safetensors(path: Path, name: str, rows: int, cols: int, data: bytes) -> None:
    header = {name: {"dtype": "I8", "shape": [rows, cols], "data_offsets": [0, len(data)]}}
    encoded = json.dumps(header, separators=(",", ":")).encode()
    # The format asks for the header padded to a multiple of 8 with spaces.
    encoded += b" " * (-len(encoded) % 8)
    path.write_bytes(struct.pack("<Q", len(encoded)) + encoded + data)


def prune(src: Path, out: Path) -> None:
    tokenizer = json.loads((src / "tokenizer.json").read_text(encoding="utf-8"))
    model = tokenizer["model"]
    if model["type"] != "Unigram":
        sys.exit(f"expected a Unigram tokenizer, got {model['type']}")

    special = {token["content"] for token in tokenizer["added_tokens"]}
    vocab = model["vocab"]
    kept = [i for i, (piece, _) in enumerate(vocab) if piece_is_kept(piece, special)]
    new_id = {old: new for new, old in enumerate(kept)}

    header, data = read_safetensors(src / "model.safetensors")
    tensors = [name for name in header if name != "__metadata__"]
    if tensors != ["embeddings"] or header["embeddings"]["dtype"] != "I8":
        sys.exit(f"expected one int8 'embeddings' tensor, got {header}")
    rows, cols = header["embeddings"]["shape"]
    if rows != len(vocab):
        sys.exit(f"table has {rows} rows, vocabulary {len(vocab)} entries")
    start, _ = header["embeddings"]["data_offsets"]
    table = data[start:]

    model["vocab"] = [vocab[i] for i in kept]
    model["unk_id"] = new_id[model["unk_id"]]
    for token in tokenizer["added_tokens"]:
        token["id"] = new_id[token["id"]]

    out.mkdir(parents=True, exist_ok=True)
    (out / "tokenizer.json").write_text(
        json.dumps(tokenizer, ensure_ascii=False), encoding="utf-8"
    )
    pruned = b"".join(table[i * cols : (i + 1) * cols] for i in kept)
    write_safetensors(out / "model.safetensors", "embeddings", len(kept), cols, pruned)
    for name in ("config.json", "modules.json", "README.md", "parity.json"):
        shutil.copy2(src / name, out / name)

    print(f"kept {len(kept)} of {len(vocab)} tokens ({len(kept) / len(vocab):.0%}), {cols} dims")
    print(f"wrote {out}")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--src", type=Path, default=repo_root() / SRC_REL)
    parser.add_argument("--out", type=Path, default=repo_root() / OUT_REL)
    args = parser.parse_args()
    prune(args.src, args.out)
    return 0


if __name__ == "__main__":
    sys.exit(main())
