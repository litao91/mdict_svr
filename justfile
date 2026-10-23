# mdict_svr — every command you need, in one place. Run `just` to list these.

set shell := ["bash", "-uc"]

# List every recipe in this file
default:
    @just --list

# node ships an npm that always matches its own major version, while the npm on
# PATH can be an ancient one from another toolchain (miniconda's v6 refuses
# Node >=22). Default to the bundled one; override with MDICT_NPM if you must.
npm := env_var_or_default("MDICT_NPM", `
  cli="$(dirname "$(readlink -f "$(command -v node)")")/../lib/node_modules/npm/bin/npm-cli.js"
  if [ -f "$cli" ]; then printf 'node "%s"' "$cli"; else printf npm; fi
`)

# Where the packed dictionaries live, and where to bind the server.
data := env_var_or_default("MDICT_DATA", "./data")
addr := env_var_or_default("MDICT_ADDR", "127.0.0.1:8080")

# ── rust ────────────────────────────────────────────────────────────────────

# Build the debug binary into target/debug/mdict_svr
build:
    cargo build

# Build the optimised binary into target/release/mdict_svr
release:
    cargo build --release

# Rust test suite: container, decoding, lookup, @@@LINK, HTML→MD, web serving
test:
    cargo test

# Clippy and rustfmt, the way CI would see them
lint:
    cargo clippy --all-targets -- -D warnings
    cargo fmt --check

# ── dictionaries ────────────────────────────────────────────────────────────

# Import one .mdx (words only) into a packed .qmd
import MDX:
    cargo run --quiet -- import --mdx {{MDX}}

# Import one .mdx together with its companion .mdd resource store
import-res MDX MDD:
    cargo run --quiet -- import --mdx {{MDX}} --mdd {{MDD}}

# Import every .mdx/.mdd under a directory (X.mdx → X.qmd, X.mdd → X.res.qmd)
import-dir DIR:
    cargo run --quiet -- import --mdx {{DIR}}

# Look a word up offline from a packed .qmd (format: html|md)
lookup WORD DB FORMAT="html":
    cargo run --quiet -- lookup {{WORD}} --db {{DB}} --format {{FORMAT}}

# Print a packed .qmd's metadata as JSON
info DB:
    cargo run --quiet -- info --db {{DB}}

# ── serve ──────────────────────────────────────────────────────────────────

# Serve the HTTP API for every dictionary under $MDICT_DATA (default ./data)
serve: build
    ./target/debug/mdict_svr serve --db {{data}} --addr {{addr}}

# Serve the API and the built web UI from one origin (run web-build first)
serve-web: build
    ./target/debug/mdict_svr serve --db {{data}} --web web/dist --addr {{addr}}

# ffmpeg used by /transcode to play Ogg-Speex; empty disables transcoding
ffmpeg := env_var_or_default("MDICT_FFMPEG", "web/node_modules/ffmpeg-static/ffmpeg")

# Serve as the browser verification does: the Sound store attached explicitly
# (directory scanning skips Sound_us_uk.res.qmd) and Speex transcoding on
serve-verify: build
    #!/usr/bin/env bash
    args=(--db {{data}} --res Sound={{data}}/Sound_us_uk.res.qmd --web web/dist --addr {{addr}})
    [ -x "{{ffmpeg}}" ] && args+=(--ffmpeg "{{ffmpeg}}")
    ./target/debug/mdict_svr serve "${args[@]}"

# ── web frontend ────────────────────────────────────────────────────────────

# Install the frontend dependencies
web-install:
    cd web && {{npm}} install

# Dev server on :5173 with a catch-all proxy to the Rust API on :8080
web-dev:
    cd web && {{npm}} run dev

# Production build into web/dist (what serve --web hands out)
web-build:
    cd web && {{npm}} run build

# Frontend unit tests (vitest + jsdom, real records from ./data)
web-test:
    cd web && {{npm}} test

# svelte-check over the whole frontend
web-check:
    cd web && {{npm}} run check

# Headless-Chromium end-to-end pass; needs a running `just serve-verify`
web-test-browser:
    cd web && {{npm}} run test:browser

# ── everything ──────────────────────────────────────────────────────────────

# Build the binary and the frontend bundle
all: build web-build

# Every check: Rust tests, frontend unit tests, type check
verify: test web-test web-check
