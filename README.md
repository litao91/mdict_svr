# mdict_svr

Import [MDict](https://en.wikipedia.org/wiki/MDict) dictionaries (`.mdx` words,
`.mdd` resources) into a packed, memory-mapped, binary-searchable index, then
serve word lookups over HTTP as **HTML** or **Markdown**.

Written in Rust, async with **tokio** + **axum**. The MDX/MDD reader is
implemented from scratch (MDict v2.0: UTF-16LE/UTF-8/GB18030 keys, zlib blocks,
RIPEMD128 keyword-index obfuscation).

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
# words only
mdict_svr import --mdx oxfordPrimary.mdx -o dict.qmd

# words + resources (images/css/js referenced by the HTML)
mdict_svr import --mdx oxfordPrimary.mdx --mdd oxfordPrimary.mdd \
    -o dict.qmd --res-out res.qmd
```

`import` streams records, so a multi-hundred-MB `.mdd` imports without loading it
all into RAM. Resource output defaults to `<out stem>.res.qmd`.

## Look up offline

```bash
mdict_svr lookup apple --db dict.qmd --format html
mdict_svr lookup apple --db dict.qmd --format md
```

## Serve

Serve one or more dictionaries. Each `--db` is `NAME=PATH` (or a bare `PATH`, in
which case the name defaults to the file stem). Attach a resource store to a
dictionary with `--res NAME=PATH`.

```bash
# a single dictionary
mdict_svr serve --db dict.qmd --addr 127.0.0.1:8080

# several dictionaries, each with its own resources
mdict_svr serve \
    --db oxford=dict.qmd     --res oxford=res.qmd \
    --db collins=collins.qmd --res collins=collins.res.qmd \
    --addr 127.0.0.1:8080
```

### HTTP API

| Method & path                             | Description                                                       |
| ----------------------------------------- | ----------------------------------------------------------------- |
| `GET /lookup?word=<w>&dict=<name>&format=html\|md` | Definition of `<w>`. `dict` optional; `format` defaults to `html`. |
| `GET /word/<w>?dict=<name>&format=html\|md`        | Same, word in the path.                                    |
| `GET /res/<dict>/<path>`                  | A resource from that dictionary's `.mdd` (correct `Content-Type`)  |
| `GET /dicts`                              | JSON list of loaded dictionaries and their metadata                |
| `GET /info`                               | Alias for `/dicts`                                                 |
| `GET /health`                             | Liveness                                                           |

When `dict` is omitted, **all** dictionaries are searched in the order they were
given:

* one match → that dictionary's definition is returned as-is;
* several matches → they are concatenated, labeled per dictionary (`<section
  data-dict="name">` in HTML, `## name` in Markdown).

Either way the responding dictionary name(s) are in the `X-Dictionary` response
header. `format` may also be implied by `Accept: text/markdown`. Markdown is
produced by converting the stored HTML at query time (in a blocking task), so the
database keeps a single source of truth. A missing word returns `404` JSON; an
unknown `dict` returns `404` JSON.

```bash
curl 'http://127.0.0.1:8080/dicts'
curl 'http://127.0.0.1:8080/lookup?word=apple&format=md'          # search all
curl 'http://127.0.0.1:8080/lookup?word=apple&dict=oxford'        # one dict, HTML
curl -i 'http://127.0.0.1:8080/lookup?word=zzz'                   # 404 JSON
curl 'http://127.0.0.1:8080/res/oxford/xiaoxuesheng.css'          # a resource
```

## Tests

```bash
cargo test
```

Covers RIPEMD128 known-answer vectors, the index cipher round-trip, zlib/stored
block decoding, `qmd` round-trip (including duplicate keys and a >64 KiB record
that would overflow qingjian's `u16` length), key normalization, and HTML→MD.

## Notes / limitations

- Targets MDX **v2.0** (the common modern layout). v1.x is parsed but less
  exercised.
- Blocks compressed with **LZO** are rejected with a clear error (the sample and
  most v2.0 dictionaries use zlib).
- Passcode-encrypted keyword headers (`Encrypted` bit `0x01`, Salsa20 + a
  registration code) are not supported; the common `Encrypted="2"`
  (RIPEMD128 index obfuscation) is.
- The `qmd` database is immutable; re-run `import` to rebuild.
