# mdict_svr API reference

This document describes everything a client (or operator) needs: the HTTP API,
the CLI that produces and serves the databases, the on-disk `qmd` container, and
the exact request/response shapes, status codes, headers, and error bodies.

- [Overview](#overview)
- [Concepts](#concepts)
- [Conventions](#conventions)
- [HTTP endpoints](#http-endpoints)
  - [`GET /health`](#get-health)
  - [`GET /dicts`](#get-dicts)
  - [`GET /info`](#get-info)
  - [`GET /lookup`](#get-lookup)
  - [`GET /word/{word}`](#get-wordword)
  - [`GET /res/{dict}/{path}`](#get-resdictpath)
- [Output formats](#output-formats)
- [Multi-dictionary resolution](#multi-dictionary-resolution)
- [Error reference](#error-reference)
- [CLI reference](#cli-reference)
- [Appendix A: the `qmd` container](#appendix-a-the-qmd-container)
- [Appendix B: MDict support matrix](#appendix-b-mdict-support-matrix)
- [Appendix C: worked example](#appendix-c-worked-example)

---

## Overview

`mdict_svr` imports MDict dictionaries (`.mdx` for words, `.mdd` for the images /
CSS / audio those words reference) into an immutable, memory-mapped,
binary-searchable container (`.qmd`), then serves lookups over HTTP. A word's
definition can be returned as the dictionary's original **HTML** or converted to
**Markdown** on request. One server can host **many dictionaries** at once and
search across them.

There is no API key, no request body, and no state: every endpoint is a `GET`.

## Concepts

| Term | Meaning |
| --- | --- |
| **dictionary** | A word store imported from a `.mdx` into a `.qmd`. Identified in the server by a **name** you choose at launch. |
| **resource store** | A `.mdd` imported into a `.qmd` (flag `is_resource`), attached to a dictionary by name. Holds images/CSS/JS/audio referenced by definitions. |
| **entry / record** | An *entry* is a unique headword (index key). A *record* is one stored payload. One entry can have several records (homographs), so `records ≥ entries`. |
| **name** | The handle used in `?dict=` and `/res/{dict}/…`. Chosen via `--db NAME=PATH` (defaults to the file stem). |
| **format** | `html` (default) or `md`/`markdown`. See [Output formats](#output-formats). |

### Key normalization

Lookups and stored keys are normalized identically, so matching is predictable:

- Surrounding whitespace is trimmed.
- **Words**: lowercased unless the source dictionary declared
  `KeyCaseSensitive="Yes"` (persisted in the container's metadata and reported by
  `/dicts` as `case_sensitive`).
- **Resources**: backslashes folded to `/`, leading `/` stripped, and `entry://`
  or `file://` scheme prefixes removed, then lowercased. So
  `entry://\Img\X.PNG`, `/img/x.png`, and `img/x.png` all resolve to `img/x.png`.

You pass the *natural* form (e.g. `?word=Apple` or `/res/oxford/img/x.png`); the
server normalizes both sides.

## Conventions

- **Base URL**: `http://<addr>` where `<addr>` is `serve --addr` (default
  `127.0.0.1:8080`). No path prefix; there is no `/v1`.
- **Method**: all endpoints are `GET`.
- **Request encoding**: query parameters must be percent-encoded
  (`?word=ice%20cream`). Path segments are URL-decoded by the server.
- **Success content types**:
  - definitions: `text/html; charset=utf-8` or `text/markdown; charset=utf-8`
  - resources: a best-effort type from the file extension
    (`mime_guess`), else `application/octet-stream`
  - metadata: `application/json`
  - health: `text/plain; charset=utf-8`
- **Error content type**: always `application/json`, body
  `{ "error": "<reason>", ...context }`.
- **Response headers**:
  - `X-Dictionary` — present on definition responses; the comma-separated
    name(s) of the dictionary(ies) that produced the result, in the order given
    to `serve`. Example: `X-Dictionary: oxford,collins`.
  - `Content-Type` as above.
- **Empty words**: `?word=` (blank/whitespace) is treated as missing.

## HTTP endpoints

### `GET /health`

Liveness probe. Always `200` once the server is up.

**Response** `200 text/plain; charset=utf-8`

```
ok
```

```bash
curl -i http://127.0.0.1:8080/health
```

---

### `GET /dicts`

Lists every loaded dictionary with its metadata. Use it to discover the valid
`?dict=` names.

**Query parameters**: none.

**Response** `200 application/json`

```json
{
  "dictionaries": [
    {
      "name": "oxford",
      "title": "牛津小学生英汉双解词典",
      "description": "Data from https://www.pdawiki.com/forum/... converted by FreeMdict",
      "creation_date": "2019-9-20",
      "source": "oxfordPrimary.mdx",
      "entries": 4275,
      "records": 4279,
      "case_sensitive": false,
      "has_resources": false
    }
  ]
}
```

| Field | Type | Description |
| --- | --- | --- |
| `name` | string | The `?dict=` handle. |
| `title` | string | Dictionary title from the MDict header (`Title`). |
| `description` | string | Header `Description` (HTML entities decoded). |
| `creation_date` | string | Header `CreationDate`, verbatim. |
| `source` | string | Source file name the `.qmd` was imported from. |
| `entries` | integer | Unique headwords (index keys). |
| `records` | integer | Stored payloads (≥ `entries`). |
| `case_sensitive` | boolean | Whether lookups preserve case. |
| `has_resources` | boolean | Whether a resource store is attached. |

```bash
curl -s http://127.0.0.1:8080/dicts | jq
```

---

### `GET /info`

Alias for [`GET /dicts`](#get-dicts) — identical response.

---

### `GET /lookup`

Primary lookup. Returns the definition of a word as HTML or Markdown.

**Query parameters**

| Name | Required | Default | Description |
| --- | --- | --- | --- |
| `word` | **yes** | — | The headword. Percent-encode spaces/punctuation. Blank → `400`. |
| `dict` | no | *all* | Restrict to one dictionary by name. Unknown name → `404`. Omit to search every dictionary. |
| `format` | no | `html` | `html`, or `md`/`markdown`/`text/markdown`/`text/x-markdown` for Markdown. Also inferred from `Accept`. |

**Responses**

- `200 text/html; charset=utf-8` — the definition HTML (see
  [Multi-dictionary resolution](#multi-dictionary-resolution) for how several
  matches are combined). Includes `X-Dictionary`.
- `200 text/markdown; charset=utf-8` — the same, converted to Markdown. Includes
  `X-Dictionary`.
- `400 application/json` — `word` missing/blank.
- `404 application/json` — no dictionary has the word, **or** `dict` names an
  unknown dictionary.
- `503 application/json` — the server was started with no dictionaries.

**Examples**

```bash
# HTML (default), searching all dictionaries
curl -i 'http://127.0.0.1:8080/lookup?word=apple'

# Markdown from one dictionary
curl -s 'http://127.0.0.1:8080/lookup?word=apple&dict=oxford&format=md'
```

`200` Markdown body (single match):

```markdown
[**apple**](sound://audio/apple.mp3) /ˈæpl/  
**noun** 名词 **plural** 复数形式 [**apples**](sound://audio/apples.mp3)  
a round, crisp, juicy fruit  
[苹果](sound://audio/苹果.mp3)
```

`404` body:

```json
{ "error": "not found", "word": "zzzqqq" }
```

`404` body (bad `dict`):

```json
{ "error": "unknown dictionary", "dict": "nope" }
```

`400` body:

```json
{ "error": "missing 'word' query parameter" }
```

`503` body:

```json
{ "error": "no dictionaries loaded" }
```

---

### `GET /word/{word}`

Same as [`GET /lookup`](#get-lookup) with the word in the path instead of the
query string. Honors the same `dict` and `format` **query** parameters.

```
GET /word/apple?format=md
GET /word/apple?dict=oxford
```

The `{word}` segment is URL-decoded, so `ice%20cream` becomes `ice cream`. For
words with `?`, `#`, `&`, or `+`, prefer `/lookup?word=…` (query encoding is
unambiguous). Responses, status codes, and `X-Dictionary` are identical to
`/lookup`. A missing word is not possible here (it is the path), so there is no
`400`.

---

### `GET /res/{dict}/{path}`

Serves one resource (image, CSS, JS, audio…) from the resource store attached to
`{dict}`. This is what makes returned HTML render with its images and styles: the
definition references assets like `xiaoxuesheng.css` or `img/apple.png`, and a
browser resolves them against this endpoint.

**Path parameters**

| Name | Description |
| --- | --- |
| `dict` | Dictionary name whose resources to use. |
| `path` | Resource path (wildcard; may contain `/`). Normalized as described in [Key normalization](#key-normalization). |

**Responses**

- `200` with the raw bytes and a `Content-Type` guessed from the extension
  (e.g. `text/css`, `image/png`, `audio/mpeg`), else `application/octet-stream`.
- `404 application/json`:
  - `{ "error": "unknown dictionary", "dict": "<name>" }`
  - `{ "error": "dictionary has no resources", "dict": "<name>" }`
  - `{ "error": "resource not found", "dict": "<name>", "path": "<path>" }`

**Example**

```bash
curl -i 'http://127.0.0.1:8080/res/oxford/xiaoxuesheng.css'
```

> Resources require importing the companion `.mdd` and launching the server with
> `--res`. Without it, every `/res/...` returns the "no resources" `404`.

---

## Output formats

### HTML (`format=html`, default)

The dictionary's original record bytes, served verbatim as
`text/html; charset=utf-8`. Records are UTF-8 for modern dictionaries. When one
entry has multiple records (homographs) they are concatenated with `\n<hr/>\n`.

### Markdown (`format=md`)

Each matching dictionary's HTML is converted with `html2md` **at query time** (in
a blocking task, so it never stalls the async runtime). The database stores only
HTML — Markdown is derived, keeping a single source of truth. Conversion is
best-effort: dictionary markup varies, so the result is a readable approximation,
not a lossless round-trip. Links, phonetics, and part-of-speech labels survive
well; exotic styling does not.

### Content negotiation

`format` wins when present. Otherwise, an `Accept` header containing
`text/markdown` or `text/x-markdown` selects Markdown; anything else (including
`text/html` and `*/*`) selects HTML.

```bash
curl -H 'Accept: text/markdown' 'http://127.0.0.1:8080/lookup?word=book'
```

## Multi-dictionary resolution

When `dict` is **omitted**, dictionaries are searched in the order they were
passed to `serve`:

1. **No match** → `404 { "error": "not found", "word": ... }`.
2. **Exactly one dictionary matches** → its definition is returned as-is (no
   wrapper), and `X-Dictionary` names it.
3. **Several dictionaries match** → results are concatenated, each labeled:
   - HTML: `<section class="dict" data-dict="NAME"><h3>NAME</h3>…</section>`,
     sections joined by `\n<hr/>\n`.
   - Markdown: `## NAME\n\n<body>`, blocks joined by `\n\n---\n\n`.
   - `X-Dictionary` lists all matching names, comma-separated, in search order.

When `dict=NAME` is given, only that dictionary is searched (unknown name →
`404`), and a single match is returned unwrapped.

## Error reference

| Status | When | Body |
| --- | --- | --- |
| `400` | `/lookup` with missing/blank `word` | `{"error":"missing 'word' query parameter"}` |
| `404` | word absent from the selected/all dictionaries | `{"error":"not found","word":"<w>"}` |
| `404` | `?dict=` names an unknown dictionary | `{"error":"unknown dictionary","dict":"<name>"}` |
| `404` | `/res/{dict}/...` unknown dictionary | `{"error":"unknown dictionary","dict":"<name>"}` |
| `404` | `/res/{dict}/...` dictionary has no `.mdd` | `{"error":"dictionary has no resources","dict":"<name>"}` |
| `404` | `/res/{dict}/{path}` path not in the store | `{"error":"resource not found","dict":"<name>","path":"<path>"}` |
| `503` | server started with zero dictionaries | `{"error":"no dictionaries loaded"}` |

All errors are `application/json`.

## CLI reference

```
mdict_svr <COMMAND>
```

### `import`

Read an MDict file and pack it into a `.qmd` container.

```
mdict_svr import --mdx <FILE> [-o <OUT>] [--mdd <FILE>] [--res-out <OUT>]
```

| Flag | Required | Description |
| --- | --- | --- |
| `--mdx <FILE>` | yes | Source word dictionary. |
| `-o, --out <FILE>` | yes | Output `.qmd` for the words. |
| `--mdd <FILE>` | no | Companion resource file to import too. |
| `--res-out <FILE>` | no | Output `.qmd` for resources. Defaults to `<out stem>.res.qmd`. Giving `--res-out` without `--mdd` is an error. |

Records are streamed, so a multi-hundred-MB `.mdd` imports without loading it all
into RAM. Prints the detected version/encoding/encryption and the resulting
counts, e.g.:

```
imported dictionary: title="牛津小学生英汉双解词典" version=2 encoding=Utf8 encrypted=2 entries=4279 -> keys=4275 slots=4279 text_bytes=5944800
wrote dict.qmd
```

### `lookup`

Offline query against one `.qmd` (no server needed). Prints to stdout.

```
mdict_svr lookup <WORD> --db <FILE> [--format html|md]
```

Exits `2` and prints `not found: <WORD>` to stderr when the word is absent.

### `serve`

Run the HTTP server with one or more dictionaries.

```
mdict_svr serve --db <NAME=PATH|PATH|DIR> [--db ...] [--res <NAME=PATH|PATH|DIR> ...] [--addr <ADDR>]
```

| Flag | Required | Description |
| --- | --- | --- |
| `--db <spec>` | yes (repeatable) | A dictionary. `NAME=PATH`, a bare `PATH` to a `.qmd` (name = file stem), **or a directory** (scanned recursively for `*.qmd`). |
| `--res <spec>` | no (repeatable) | A resource store. `NAME=PATH` attaches to dictionary `NAME`; a bare `PATH` attaches to the sole dictionary (ambiguous with several → error); a directory is scanned for `*.res.qmd`. |
| `--addr <ADDR>` | no | Bind address, default `127.0.0.1:8080`. |

**Directory scanning.** When a `--db` (or `--res`) path is a directory it is
walked recursively and every `*.qmd` is served, so you can point the server at a
folder of dictionaries:

- `X.qmd` → a word dictionary named `X` (the file stem).
- `X.res.qmd` → the resource store for dictionary `X`, attached automatically.

Names must be unique; if two files in different subdirectories share a stem, the
second is named by its path relative to the scanned directory (separators → `-`).
Discovered `*.res.qmd` with no matching dictionary are skipped with a warning;
explicit `--res NAME=` that matches nothing is an error. Giving `NAME=<directory>`
is rejected (a directory holds many dictionaries). Duplicate names across specs
are rejected. Shuts down cleanly on `Ctrl-C`.

```bash
# serve everything under a directory
mdict_svr serve --db /var/lib/mdict --addr 0.0.0.0:8080
```

```
mdict_svr serve \
  --db oxford=dict.qmd     --res oxford=res.qmd \
  --db collins=collins.qmd --res collins=collins.res.qmd \
  --addr 0.0.0.0:8080
```

### `info`

Print one `.qmd`'s metadata as JSON.

```
mdict_svr info --db <FILE>
```

```json
{
  "path": "dict.qmd",
  "title": "牛津小学生英汉双解词典",
  "description": "...",
  "creation_date": "2019-9-20",
  "source": "oxfordPrimary.mdx",
  "is_resource": false,
  "case_sensitive": false,
  "entries": 4275,
  "records": 4279
}
```

## Appendix A: the `qmd` container

Immutable, little-endian, memory-mapped. Modeled on qingjian's `Dictionary`
(arenas + a key-sorted index table + a parallel slot table) but with **64-bit**
offsets/lengths so a single record can be an arbitrarily large HTML page and the
arenas can exceed 4 GiB.

```
offset  size  section
0       128   header
128     ..    TEXT   record payloads, concatenated (streamed at import)
        ..    KEYS   normalized keys, concatenated, sorted ascending by bytes
        ..    INDX   KeyIndex[24] * num_keys, parallel to KEYS order
        ..    SLOT   Slot[24]     * num_slots, grouped to match INDX
        ..    META   JSON metadata blob
```

Header (128 bytes, LE):

```
0   [4]  magic "QMD1"
4   u32  format version (=1)
8   u32  flags (bit0 = is_resource)
12  u32  reserved
16  u64  num_keys
24  u64  num_slots
32  u64 keys_off    40 u64 keys_len
48  u64 text_off    56 u64 text_len
64  u64 index_off   72 u64 index_len   (= num_keys*24)
80  u64 slot_off    88 u64 slot_len    (= num_slots*24)
96  u64 meta_off   104 u64 meta_len
112 [16] reserved (zero)
```

`KeyIndex` (24 bytes): `key_start:u64, first_slot:u64, key_len:u32, slot_count:u32`.
`Slot` (24 bytes): `text_start:u64, text_len:u64, kind:u32, _pad:u32`
(`kind`: `0` = HTML, `1` = resource).

Lookup is a binary search over `INDX` by key bytes, then reading the
`[first_slot, first_slot+slot_count)` run of `SLOT` and slicing `TEXT`. Search
cost is independent of record size; payloads are touched only on a hit. All
section offsets are 8-byte aligned and bounds-checked at open. Reading uses plain
slice indexing — no `unsafe` beyond the initial `mmap`.

## Appendix B: MDict support matrix

| Feature | Status |
| --- | --- |
| MDX/MDD v2.0 | Supported (primary target) |
| MDX v1.x | Parsed; less exercised |
| Key encoding UTF-16LE / UTF-8 / GB18030 | Supported |
| Block compression: stored, zlib | Supported |
| Block compression: LZO | **Not supported** — rejected with a clear error |
| `Encrypted="2"` (RIPEMD128 keyword-index obfuscation) | Supported |
| `Encrypted="0"` (none) | Supported |
| `Encrypted` bit `0x01` (Salsa20 keyword header + registration passcode) | **Not supported** — import fails with a clear error |
| Duplicate headwords | Supported (`slot_count` > 1) |
| `KeyCaseSensitive`, `StripKey` | Honored |

The reader verifies adler32 checksums on the header and every block, so corrupt
or mis-parsed data fails loudly rather than returning garbage.

## Appendix C: worked example

```bash
# 1. Import words (and resources if you have the .mdd)
mdict_svr import --mdx oxfordPrimary.mdx -o oxford.qmd
#    with resources:
# mdict_svr import --mdx oxfordPrimary.mdx --mdd oxfordPrimary.mdd \
#     -o oxford.qmd --res-out oxford.res.qmd

# 2. Serve two dictionaries
mdict_svr serve --db oxford=oxford.qmd --res oxford=oxford.res.qmd \
                --db backup=oxford.qmd --addr 127.0.0.1:8080 &

# 3. Discover what is loaded
curl -s localhost:8080/dicts | jq '.dictionaries[].name'
# "oxford"
# "backup"

# 4. Look up a word (searches all; both match -> combined, labeled)
curl -sD - 'localhost:8080/lookup?word=apple&format=md' | grep -i x-dictionary
# x-dictionary: oxford,backup

# 5. Scope to one dictionary, get HTML
curl -s 'localhost:8080/lookup?word=apple&dict=oxford' | head -c 120

# 6. Fetch a referenced asset so the HTML renders
curl -sI 'localhost:8080/res/oxford/xiaoxuesheng.css' | grep -i content-type

# 7. Misses
curl -s 'localhost:8080/lookup?word=zzz'      # {"error":"not found","word":"zzz"}
curl -s 'localhost:8080/lookup?word=apple&dict=nope'  # {"error":"unknown dictionary","dict":"nope"}
```
