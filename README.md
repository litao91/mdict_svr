# mdict_svr

Import [MDict](https://en.wikipedia.org/wiki/MDict) dictionaries (`.mdx` words,
`.mdd` resources) into a packed, memory-mapped, binary-searchable index, then
serve word lookups over HTTP as **HTML** or **Markdown**.

Written in Rust, async with **tokio** + **axum**. The MDX/MDD reader is
implemented from scratch (MDict v1.x and v2.0: UTF-16LE/UTF-8/GB18030 keys,
zlib **and LZO1X** blocks, RIPEMD128 keyword-index obfuscation).

Full request/response reference: **[docs/API.md](docs/API.md)**.

## The packed index (`qmd`)

Modeled on qingjian's `Dictionary`: two byte arenas (keys, record payloads) plus
a key-sorted index table (binary search) and a parallel slot table, all in one
file that is `mmap`'d read-only. Unlike qingjian's `u16`/`u32` fields, offsets and
lengths here are **64-bit**, so a single record can be an arbitrarily large HTML
page and the arena can exceed 4 GiB.

```
[0..128)  header: magic "QMD1", counts, section offsets/lengths
TEXT      record payloads (streamed to disk during import)
KEYS      normalized keys, sorted ascending
INDX      KeyIndex[24] { key_start:u64, first_slot:u64, key_len:u32, slot_count:u32 }
SLOT      Slot[24]     { text_start:u64, text_len:u64, kind:u32, _pad:u32 }
META      JSON metadata (title, description, counts, ...)
```

Lookup = binary search `INDX` by normalized key, then read the slot run and slice
`TEXT`. Search cost is independent of record size; big payloads are touched only
on a hit. Duplicate headwords are supported via `slot_count`.

## Build

```bash
cargo build --release
```

## Import

```bash
# a single dictionary (words only); -o is optional (defaults to <source>.qmd)
mdict_svr import --mdx oxfordPrimary.mdx -o dict.qmd

# words + resources (images/css/js referenced by the HTML)
mdict_svr import --mdx oxfordPrimary.mdx --mdd oxfordPrimary.mdd \
    -o dict.qmd --res-out res.qmd

# a whole directory: recursively import every .mdx/.mdd under it.
# X.mdx -> X.qmd, sibling X.mdd -> X.res.qmd, mirroring subpaths.
# -o is the output directory (defaults to the input directory).
mdict_svr import --mdx /path/to/dicts -o /var/lib/mdict
```

`import` streams records, so a multi-hundred-MB `.mdd` imports without loading it
all into RAM. In single-file mode the resource output defaults to
`<out stem>.res.qmd`; `--mdd`/`--res-out` are single-file only (directory mode
pairs `.mdx`/`.mdd` by name automatically). The output naming matches what
`serve --db <dir>` discovers, so you can import a directory and serve it directly.

## Look up offline

```bash
mdict_svr lookup apple --db dict.qmd --format html
mdict_svr lookup apple --db dict.qmd --format md
```

## Serve

Serve one or more dictionaries. Each `--db` is `NAME=PATH`, a bare `PATH` to a
`.qmd` (name defaults to the file stem), **or a directory** — a directory is
scanned recursively and every `*.qmd` under it is served: `X.qmd` becomes a
dictionary named `X`, and `X.res.qmd` is attached as its resource store. Attach
resources explicitly with `--res NAME=PATH` (or a directory).

```bash
# a single dictionary
mdict_svr serve --db dict.qmd --addr 127.0.0.1:8080

# every dictionary under a directory (words + their *.res.qmd resources)
mdict_svr serve --db /var/lib/mdict --addr 127.0.0.1:8080

# several dictionaries, each with its own resources
mdict_svr serve \
    --db oxford=dict.qmd     --res oxford=res.qmd \
    --db collins=collins.qmd --res collins=collins.res.qmd \
    --addr 127.0.0.1:8080
```

### HTTP API

| Method & path                             | Description                                                       |
| ----------------------------------------- | ----------------------------------------------------------------- |
| `GET /lookup?word=<w>&dict=<name>&format=html\|md\|json` | Definition of `<w>`. `dict` optional; `format` defaults to `html`. |
| `GET /word/<w>?dict=<name>&format=html\|md\|json`        | Same, word in the path.                                    |
| `GET /suggest?q=<partial>&dict=<name>&limit=<n>` | Headwords that start with `<partial>` (as-you-type autocomplete); JSON. |
| `GET /res/<dict>/<path>`                  | A resource from that dictionary's `.mdd` (correct `Content-Type`)  |
| `GET /<path>`                             | Asset fallback: serves a root-relative resource referenced by a definition |
| `GET /dicts`                              | JSON list of loaded dictionaries and their metadata                |
| `GET /info`                               | Alias for `/dicts`                                                 |
| `GET /health`                             | Liveness                                                           |

Definitions are rewritten on the fly so their assets and links work in a browser:
`sound://X` and `file://X` → `/res/<dict>/X`, `entry://W` → `/lookup?word=W`, and
a `<base href="/">` is injected so scheme-less refs (`src="txbapple.jpg"`,
`/media/…/fruit.jpg`) resolve at the origin root and are served by the asset
fallback from the `.mdd`.

When `dict` is omitted, **all** dictionaries are searched in the order they were
given:

* one match → that dictionary's definition is returned as-is;
* several matches → they are concatenated, labeled per dictionary (`<section
  data-dict="name">` in HTML, `# name` in Markdown).

Either way the responding dictionary name(s) are in the `X-Dictionary` response
header. `format` may also be implied by `Accept: text/markdown` or
`Accept: application/json`. Markdown is produced by converting the stored HTML at
query time (in a blocking task), so the database keeps a single source of truth.
`format=json` wraps the same HTML in a JSON envelope
(`{"word","format","dictionaries","html"}`) for programmatic clients. A missing
word returns `404` JSON; an unknown `dict` returns `404` JSON.

```bash
curl 'http://127.0.0.1:8080/dicts'
curl 'http://127.0.0.1:8080/lookup?word=apple&format=md'          # search all
curl 'http://127.0.0.1:8080/lookup?word=apple&format=json'        # JSON envelope
curl 'http://127.0.0.1:8080/lookup?word=apple&dict=oxford'        # one dict, HTML
curl 'http://127.0.0.1:8080/suggest?q=app&limit=10'               # autocomplete
curl -i 'http://127.0.0.1:8080/lookup?word=zzz'                   # 404 JSON
curl 'http://127.0.0.1:8080/res/oxford/xiaoxuesheng.css'          # a resource
```

`/suggest` returns JSON `{ "query", "count", "suggestions": [headword, …] }`,
scanning the byte-sorted key index (binary search to the prefix range, then a
short walk) so cost is independent of record size. It searches all dictionaries
by default (deduped, sorted), or one with `dict=`; `limit` defaults to 20 and is
capped at 100. An empty `q` returns the alphabetically first headwords.

## Web UI

`web/` is a Svelte 5 + Vite frontend for the API above: per-dictionary tabs,
pronunciation audio that plays in place, cross-references that navigate without a
reload, hover previews, an image lightbox, a command palette, find-in-entry, and
light/dark themes. It sanitizes the stored publisher markup before injecting it
(their `<base>`, `<link>` and 55k inline `onclick` handlers never reach the DOM)
and remaps the publishers' inline colours so every dictionary stays legible on a
dark background.

Audio that no browser can decode is handled up front: CALD4 and the Sound
libraries store Ogg-Speex `.spx`, which the server reports as `200 audio/ogg` and
`<audio>` then fails to *decode*. With `serve --ffmpeg <PATH>` the player asks
`GET /transcode/{path}` to re-encode the clip to WAV on first play (cached after
that); without a decoder configured it explains the codec and offers the file as
a download instead of failing silently.

```bash
cd web
npm install
npm run dev          # http://127.0.0.1:5173, proxies everything else to :8080
npm run build        # → web/dist
npm test             # vitest + jsdom unit tests
npm run check        # svelte-check
npm run test:browser # headless-Chromium pass; needs a running `serve --web`
```

Serve the build from the Rust binary so one command gives one origin:

```bash
mdict_svr serve --db ./data --web web/dist --addr 127.0.0.1:8080 \
    --ffmpeg $(command -v ffmpeg)   # optional: play Ogg-Speex by transcoding
```

`--web` adds exactly three routes — `/`, `/index.html` (the shell, `no-cache`) and
`/assets/*` (hashed output, immutable) — and both fall through to the asset
fallback when the file is not in the build directory, so a dictionary asset
literally keyed `index.html` or under `assets/` still resolves. The app keeps its
state in the query string (`/?word=apple&dict=OALD9`) precisely so it cannot
collide with dictionary asset paths. With `--web` set, opening `/word/apple` or
`/lookup?word=apple` in a real browser (`Sec-Fetch-Dest: document`) redirects
into the app; `curl` and `fetch()` are unaffected. See `docs/API.md`.

## Tests

```bash
cargo test          # Rust: container, decoding, lookup, HTML→MD, web serving
cd web && npm test  # frontend: sanitizer, URL rewriting, colours, API outcomes
```

The Rust suite covers RIPEMD128 known-answer vectors, the index cipher
round-trip, zlib/stored block decoding, `qmd` round-trip (including duplicate keys
and a >64 KiB record that would overflow qingjian's `u16` length), key
normalization, `@@@LINK` redirect resolution, and HTML→MD. The Markdown tests are
regression tests built from real snippets of each dictionary in `./data`; see
`src/md/tests.rs`. The frontend tests do the same for the sanitizer: real records
from all five markup vocabularies, asserting the publisher stylesheet, scripts,
`<base>` and inline handlers are gone while layout styles and readable text
survive.

## Notes / limitations

- Supports MDX **v1.x and v2.0**. Record HTML is decoded by sniffing the bytes,
  so both UTF-8 (modern v2.0) and UTF-16LE (many v1.x, e.g. Collins COBUILD)
  dictionaries render correctly.
- Block compression: stored, **zlib**, and **LZO1X** are all supported.
- Passcode-encrypted keyword headers (`Encrypted` bit `0x01`, Salsa20 + a
  registration code) are not supported; the common `Encrypted="2"`
  (RIPEMD128 index obfuscation) is.
- The `qmd` database is immutable; re-run `import` to rebuild.
- Entries stored as `@@@LINK=<target>` redirect stubs (29% of OALD9) are followed
  to their target, so inflected forms return the headword's definition.
- Markdown is derived at query time and is a faithful *reading* of the record,
  not a lossless one: colour, font and size have no Markdown equivalent and are
  dropped, as is the web-app chrome the dictionary's own stylesheet hides. Two
  source-data defects are visible in the output and are not conversion bugs — the
  新牛津英汉双解大词典 writes phonetics in a proprietary `Kingsoft Phonetic Plain`
  font (so `/rQn/` only looks like /rʌn/ in that font), and some `col` records are
  double-encoded UTF-8.
