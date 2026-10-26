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
  - [`GET /suggest`](#get-suggest)
  - [`GET /res/{dict}/{path}`](#get-resdictpath)
  - [`GET /{path}` (asset fallback)](#get-path-asset-fallback)
  - [How definition URLs are made to work](#how-definition-urls-are-made-to-work)
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
| **format** | `html` (default), `md`/`markdown`, or `json`. See [Output formats](#output-formats). |

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

### Redirects

Some MDict dictionaries store an entry as a bare `@@@LINK=<target>` control string
instead of HTML — OALD9 does this for 24,298 of its records (29%), covering
inflected forms and variants, so `'ll` points at `will` and `abased` at `abase`.
Lookups follow these to the target entry and return *its* definition, in every
format. The chain is deduplicated and cycle-guarded, so a key whose slots all
point at the same target yields one copy, and a dictionary that points a key at
itself terminates instead of spinning.

If a target cannot be resolved, the stub is returned rather than dropped: as HTML
it is the raw `@@@LINK=` text, and as Markdown it renders as `→ <target>`.

## Conventions

- **Base URL**: `http://<addr>` where `<addr>` is `serve --addr` (default
  `127.0.0.1:8080`). No path prefix; there is no `/v1`.
- **Method**: all endpoints are `GET`.
- **Request encoding**: query parameters must be percent-encoded
  (`?word=ice%20cream`). Path segments are URL-decoded by the server.
- **Success content types**:
  - definitions: `text/html; charset=utf-8`, `text/markdown; charset=utf-8`, or
    `application/json` (for `format=json`)
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
| `format` | no | `html` | `html`, `md`/`markdown`/`text/markdown`/`text/x-markdown` for Markdown, or `json`/`application/json`. Also inferred from `Accept`. |

**Responses**

- `200 text/html; charset=utf-8` — the definition HTML (see
  [Multi-dictionary resolution](#multi-dictionary-resolution) for how several
  matches are combined). Includes `X-Dictionary`.
- `200 text/markdown; charset=utf-8` — the same, converted to Markdown. Includes
  `X-Dictionary`.
- `200 application/json` — a JSON envelope around the same HTML. Includes
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

# JSON envelope (html as a field)
curl -s 'http://127.0.0.1:8080/lookup?word=apple&format=json'
```

`200` JSON body (`format=json`):

```json
{
  "word": "apple",
  "format": "html",
  "dictionaries": ["oxford"],
  "html": "<base href=\"/\">\n<a href=\"/res/oxford/audio/apple.mp3\">…</a> …"
}
```

`html` is byte-for-byte the body the `format=html` request would return
(including the injected `<base href="/">` and, for multiple matches, the
`<section data-dict="…">` wrappers). `dictionaries` mirrors the `X-Dictionary`
header as an array.

`200` Markdown body (single match):

```markdown
**apple**  
**apple** [audio](/res/oxford/ukappen014.spx) [audio](/res/oxford/apple.spx) /ˈæp.l̩/ **noun** [**C** or **U**]  
![txbapple](txbapple.jpg)

► **A1** a round fruit with firm, white flesh and a green, red, or yellow skin:
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

### `GET /suggest`

As-you-type autocomplete: given a partial input, return the headwords that begin
with it. Backed by a prefix scan of the byte-sorted key index (binary search to
the range, then a short walk), so cost is independent of record size and never
touches the TEXT arena.

**Query parameters**

| Name | Required | Default | Description |
| --- | --- | --- | --- |
| `q` | no | `""` | The partial input. `word` and `prefix` are accepted as aliases (first present wins). Missing/empty returns the alphabetically first headwords. |
| `dict` | no | *all* | Restrict to one dictionary by name. Unknown name → `404`. Omit to search every dictionary. |
| `limit` | no | `20` | Max suggestions, clamped to `1..=100`. |
| `fuzzy` | no | `false` | Also run a typo-tolerant pass (Levenshtein ≤ 2, closest first, ties by shared prefix) when the input is at least 4 characters and is not itself a headword. Scans every key, so leave it off for keystrokes and turn it on once the input settles. |

**Response** `200 application/json`

```json
{
  "query": "app",
  "count": 8,
  "mode": "prefix",
  "fuzzy": [],
  "suggestions": [
    "apparatus", "appear", "appearance", "appendix",
    "appetite", "applaud", "apple", "appreciate"
  ]
}
```

- `mode` says how the list was produced: `prefix`, `wildcard` (the query
  contains `*` or `?`, matched against every headword), `fuzzy`, or
  `prefix+fuzzy` when both contributed.
- `fuzzy` carries only the typo-tolerant additions, in distance order, so a
  client can present them as "did you mean" instead of mixing them into the
  prefix list. With `mode: "wildcard"` it is empty.

- `suggestions` are the **stored (normalized) headwords** — lowercased for
  case-insensitive dictionaries — i.e. exactly the strings `/lookup?word=…`
  expects, so a client can feed a chosen suggestion straight back.
- When searching multiple dictionaries the results are **merged, deduped, and
  sorted** into one flat list (no per-dictionary grouping). `X-Dictionary` names
  the dictionaries that were searched.
- `count` is `suggestions.length` (≤ `limit`).

**Errors**

- `404 application/json` — `dict` names an unknown dictionary.
- `503 application/json` — the server was started with no dictionaries.

Unlike `/lookup`, no-match is **not** an error: it returns `200` with an empty
`suggestions` array.

**Examples**

```bash
curl -s 'http://127.0.0.1:8080/suggest?q=app&limit=10'
curl -s 'http://127.0.0.1:8080/suggest?q=ban&dict=oxford'
curl -s 'http://127.0.0.1:8080/suggest?prefix=fruit'   # alias
curl -s 'http://127.0.0.1:8080/suggest?q=app*e'        # wildcard, full-index scan
curl -s 'http://127.0.0.1:8080/suggest?q=aple&fuzzy=true'   # "did you mean apple"

### `GET /nearby`

The headwords sitting either side of a word in one dictionary's byte-sorted
index — what a paper dictionary gives you when your thumb lands on the wrong
page. `/suggest` cannot answer this: it only scans forward from a prefix, so
"the headword before this one" is unreachable from it. This reads the index
position directly (binary search plus a short walk in each direction).

| Name | Required | Default | Description |
| --- | --- | --- | --- |
| `word` | yes | — | The headword to centre on. Need not exist in the dictionary. |
| `dict` | no | *first that has `word`, else first loaded* | Which dictionary's index to read. |
| `before` / `after` | no | `5` | Window size each way, clamped to `0..=25`. |

```json
{
  "word": "apple",
  "dict": "OALD9",
  "exact": true,
  "prev": ["applauded", "applauding", "applauds", "applause"],
  "next": ["apple cart", "apple cider", "apple pie", "apple-cheeked"]
}
```

`exact` says whether `word` is itself a headword; when it is not, the window is
the insertion point, which is what makes this a "did you mean" primitive too.
`prev` is ascending, so the immediate predecessor is its last element.

```bash
curl -s 'http://127.0.0.1:8080/nearby?word=apple&dict=OALD9&before=2&after=2'
```

**Errors** — `400` without `word`; `404` for an unknown `dict`; `503` with no
dictionaries loaded.
```

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
  - `{ "error": "resource not found", "path": "<path>" }`

**Example**

```bash
curl -i 'http://127.0.0.1:8080/res/oxford/xiaoxuesheng.css'
```

> Resources require importing the companion `.mdd` and launching the server with
> `--res` (or a directory containing `X.res.qmd`). Without a resource store, these
> return a "no resources" `404`.

---

### `GET /{path}` (asset fallback)

Any path that matches no other route is treated as a **resource reference from a
served definition** and looked up in the loaded resource stores. This is what lets
a dictionary's images/CSS/audio load: definitions reference assets by root-relative
path (e.g. `<img src="txbapple.jpg">`, `/media/english/thumb/…/fruit.jpg`), which
the browser requests from this origin.

- With several resource stores, they are searched in `serve` order; the first hit
  wins. Add `?dict=<name>` to scope the lookup to one dictionary.
- Responses are identical to [`/res/{dict}/{path}`](#get-resdictpath): `200` with
  the bytes and a guessed `Content-Type`, or `404` JSON
  (`{ "error": "no resource dictionaries loaded" }` or
  `{ "error": "resource not found", "path": "<path>" }`).

```bash
curl -i 'http://127.0.0.1:8080/media/english/thumb/f/fru/fruit/fruit.jpg'
```

### The web UI routes (only with `--web`)

With `--web <DIR>` the server also serves a single-page app from the same origin.
Only three routes are added, and only when the flag is given:

| Route | Response | Caching |
| --- | --- | --- |
| `GET /` and `GET /index.html` | the app shell | `Cache-Control: no-cache` |
| `GET /assets/{path}` | hashed build output from `<DIR>/assets/` | `max-age=31536000, immutable` |

**The app keeps all of its state in the query string** (`/?word=apple&dict=OALD9`)
and has no path-based client routes of its own. That is deliberate: definitions
reference assets at arbitrary root paths (`/media/english/thumb/…/fruit.jpg`,
`/txbapple.jpg`), so a client route like `/word/{word}` in the browser would be
ambiguous with them.

**Asset shadowing.** A dictionary could in principle store a resource literally
keyed `index.html` or under `assets/`. Both handlers therefore fall through to the
[asset fallback](#get-path-asset-fallback) when the file is not present in
`<DIR>`, so a stored asset wins over a 404 and route priority ceases to matter.
Paths are joined with a component-wise check (`..`, `.`, absolute paths and
percent-decoded traversal are all rejected) and the result is canonicalized and
required to stay inside `<DIR>`.

**Opening a raw definition in a browser.** When `--web` is set, a request to
`/word/{word}` or `/lookup` that carries `Sec-Fetch-Dest: document` (a real
top-level navigation — `curl`, `fetch()` and XHR never send it) and no explicit
`format` parameter answers `302` to `/?word=…&dict=…`. Programmatic clients see no
change:

```bash
# a browser navigation lands in the app
curl -i -H 'Sec-Fetch-Dest: document' 'http://127.0.0.1:8080/word/apple'
#   → 302, Location: /?word=apple

# a programmatic client is unaffected
curl -s 'http://127.0.0.1:8080/lookup?word=apple&format=json' | head -c 80
```

This is what makes the server's own `entry://` → `/lookup?word=` cross-references
usable when somebody opens a raw definition directly instead of the app.

### `GET /transcode/{path}`

Re-encodes a stored audio file that no browser can decode into PCM WAV
(`audio/wav`), which all of them can. In practice this is Ogg-Speex `.spx`:
CALD4, `Sound` and `Sound-en_GB` store every pronunciation in it, and the server
would otherwise serve it as `200 audio/ogg` for `<audio>` to fail at *decode*
time. Speex has neither a browser decoder nor a pure-Rust one, so this shells
out to an external ffmpeg given with `serve --ffmpeg`; without it the route
answers `501 {"error":"transcoding not configured"}` and the web UI keeps
offering the file as a download.

- The path is resolved through the resource stores exactly like the
  [asset fallback](#get-path-asset-fallback); `?dict=<name>` scopes it. There is
  no filesystem access, so there is nothing to traverse.
- Inputs above 8 MiB are refused with `413`; undecodable input is `422` with the
  decoder's stderr in `detail`.
- Results are cached in memory keyed by `dict` + path and served with
  `Cache-Control: public, max-age=604800` (the input is immutable).

```bash
curl -s 'http://127.0.0.1:8080/dicts' | jq .transcode      # is a decoder configured?
curl -s -o apple.wav 'http://127.0.0.1:8080/transcode/ukappen014.spx?dict=CALD4'
```

### How definition URLs are made to work

Definitions are rewritten **on the fly** when served as HTML (and before
Markdown conversion), so their links resolve against this server:

| In the dictionary | Rewritten to | Purpose |
| --- | --- | --- |
| `sound://X` | `/res/{dict}/X` | pronunciation audio from the `.mdd` |
| `file://X` | `/res/{dict}/X` | a resource by absolute reference |
| `entry://W` | `/lookup?word=W` | a cross-reference to headword `W` |

Scheme-less relative refs (`src="txbapple.jpg"`) are left as-is; the response is
prefixed with `<base href="/">` so they resolve at the origin root, and the
[asset fallback](#get-path-asset-fallback) serves them. `{dict}` is the name of
the dictionary the definition came from (each dictionary's HTML is rewritten with
its own name, so multi-dictionary results stay correctly scoped).

---

## Output formats

### HTML (`format=html`, default)

The dictionary's record, decoded to text and served as `text/html; charset=utf-8`.
Modern dictionaries store UTF-8; many v1.x ones (e.g. Collins COBUILD) store
UTF-16LE, which is transcoded by sniffing the bytes rather than trusting the
header. Every record's trailing NUL terminator is stripped. Otherwise the markup
is passed through untouched. When one entry has multiple records (homographs)
they are concatenated with `\n<hr/>\n`.

### Markdown (`format=md`)

Each matching dictionary's HTML is converted **at query time**, in a blocking task
so it never stalls the async runtime. The database stores only HTML — Markdown is
derived, keeping a single source of truth.

The converter is built into this crate (`src/md/`) on top of `html5ever`, and is
written against the markup the real dictionaries actually use rather than being a
general-purpose HTML→Markdown tool. In particular it:

- collapses the source's indentation and padding whitespace, so pretty-printed
  records do not turn into indented code blocks;
- treats `<br>` and `display:block` (as either an inline style or an attribute)
  as line structure, which is how most of these dictionaries are laid out;
- drops web-app chrome — `script`/`style`/`link`/`meta`, HTML comments, hidden
  subtrees, and the "Add to my wordlist" / "jump to other results" / "enlarge
  image" / "Check pronunciation" controls — while *keeping* the text of panels the
  original collapses behind a JavaScript toggle ("Word Origin", "Extra examples");
- renders pronunciation links as `[audio](…)`, and keeps the label when the link
  wraps real text rather than an icon;
- turns sense numbers, examples, cross-references, idioms and section panels into
  numbered lead-ins, bullet lists, links and headings.

Colour, font and size carry no Markdown equivalent and are dropped; the words they
styled are not.

### JSON (`format=json`)

The same HTML as `format=html`, wrapped in a JSON envelope for programmatic
clients:

```json
{
  "word": "apple",
  "format": "html",
  "dictionaries": ["oxford"],
  "html": "<base href=\"/\">\n…"
}
```

- `html` — identical to the `format=html` body (same `<base href="/">` injection
  and `<section data-dict="…">` wrappers for multiple matches).
- `dictionaries` — the matching dictionary names as an array (same values as the
  `X-Dictionary` header, which is still sent).
- `format` — always `"html"`, describing the encoding of the `html` field.
- `word` — the queried headword.

Content type is `application/json`. A miss is still the `404` JSON error, so a
client can rely on JSON for both outcomes when it asks for `format=json`.

### Content negotiation

`format` wins when present. Otherwise, an `Accept` header containing
`text/markdown` or `text/x-markdown` selects Markdown, one containing
`application/json` selects JSON, and anything else (including `text/html` and
`*/*`) selects HTML.

```bash
curl -H 'Accept: text/markdown' 'http://127.0.0.1:8080/lookup?word=book'
curl -H 'Accept: application/json' 'http://127.0.0.1:8080/lookup?word=book'
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
   - Markdown: `# NAME\n\n<body>`, blocks joined by `\n\n---\n\n`. `#` sits one
     level above the `##` the converter gives an entry's headword.
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
| `404` | resource path not in the store | `{"error":"resource not found","path":"<path>"}` |
| `404` | asset fallback with no resource store loaded | `{"error":"no resource dictionaries loaded"}` |
| `503` | server started with zero dictionaries | `{"error":"no dictionaries loaded"}` |

All errors are `application/json`.

## CLI reference

```
mdict_svr <COMMAND>
```

### `import`

Read MDict file(s) and pack them into `.qmd` container(s). `--mdx` may be a
single file **or a directory** to scan recursively.

```
mdict_svr import --mdx <FILE|DIR> [-o <OUT|OUTDIR>] [--mdd <FILE>] [--res-out <OUT>]
```

| Flag | Required | Description |
| --- | --- | --- |
| `--mdx <path>` | yes | A `.mdx` file, or a directory scanned recursively for every `.mdx`/`.mdd`. |
| `-o, --out <path>` | no | Single-file mode: output `.qmd` (default `<source stem>.qmd` beside the source). Directory mode: output **directory** (default the input directory), mirroring source subpaths. |
| `--mdd <FILE>` | no | Companion resource file. **Single-file mode only.** |
| `--res-out <FILE>` | no | Resource output (default `<out stem>.res.qmd`). **Single-file mode only.** Giving it without `--mdd` is an error. |

**Directory mode.** Every `*.mdx`/`*.mdd` under the directory is imported, so you
can convert a folder of dictionaries in one go:

- `X.mdx` → `X.qmd` (a word dictionary).
- a sibling `X.mdd` (same directory and stem, matched case-insensitively) →
  `X.res.qmd`, attached as `X`'s resources.
- a lone `X.mdd` with no matching `.mdx` → `X.res.qmd` (resource-only).

Output paths mirror the source subpaths under the output directory (so
`sub/x.mdx` → `<outdir>/sub/x.qmd`), and parent directories are created as
needed. This naming is exactly what [`serve --db <dir>`](#serve) discovers, so you
can import a directory and serve the result directly. `--mdd`/`--res-out` are
rejected in directory mode (pairing is automatic).

Records are streamed, so a multi-hundred-MB `.mdd` imports without loading it all
into RAM. Prints the detected version/encoding/encryption and the resulting
counts for each file, e.g.:

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
| `--ffmpeg <PATH>` | no | ffmpeg binary used to decode audio no browser can play (Ogg-Speex `.spx`). Defaults to `$MDICT_FFMPEG`, then `ffmpeg` on `PATH`; without any of them those files stay download-only. |
| `--web <DIR>` | no | Serve a built web UI (the `web/dist` output of the Svelte frontend) from the same origin. The directory must contain `index.html` or `serve` exits with an error. Without it the API-only behaviour is unchanged. |
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
| MDX v1.x | Supported (e.g. Collins COBUILD 8th, v1.2 LZO) |
| Key encoding UTF-16LE / UTF-8 / GB18030 | Supported |
| Record text encoding (UTF-8 / UTF-16LE / BOM) | Sniffed per record and decoded |
| Block compression: stored, zlib | Supported |
| Block compression: LZO1X | Supported |
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
