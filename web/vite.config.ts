import { defineConfig, type Plugin } from 'vitest/config'
import { svelte } from '@sveltejs/vite-plugin-svelte'

const API = process.env.MDICT_API ?? 'http://127.0.0.1:8080'

/**
 * Paths the dev server must keep for itself. Everything else is a dictionary
 * asset — definitions reference files at arbitrary root paths
 * (`/media/english/thumb/…/fruit.jpg`, `/txbapple.jpg`, `/snd_uk.png`) that only
 * the Rust server can serve, so the proxy has to be a catch-all. Forwarding just
 * the four API routes would 404 every asset in dev and silently diverge from
 * production, where the SPA and the API share an origin.
 */
const OWNED = /^\/(?:src|@|node_modules|assets|index\.html)(?:[/?]|$)/

/**
 * Vite's `server.proxy` needs a known path prefix per entry, and there is no
 * such prefix here, so the catch-all is a middleware plugin instead.
 */
function api_proxy(): Plugin {
  return {
    name: 'mdict-api-proxy',
    configureServer(server) {
      server.middlewares.use(async (req, res, next) => {
        const url = req.url ?? '/'
        if (OWNED.test(url) || (req.method ?? 'GET').toUpperCase() !== 'GET') return next()
        try {
          const upstream = await fetch(new URL(url, API), {
            // Never forward Sec-Fetch-Dest: the server 302s a "document"
            // navigation into the SPA, which would bounce the dev proxy.
            headers: { accept: '*/*' },
            redirect: 'manual',
          })
          res.statusCode = upstream.status
          upstream.headers.forEach((value, key) => {
            if (key.toLowerCase() !== 'content-encoding') res.setHeader(key, value)
          })
          res.end(Buffer.from(await upstream.arrayBuffer()))
        } catch (err) {
          res.statusCode = 502
          res.setHeader('content-type', 'application/json')
          res.end(JSON.stringify({ error: 'api unreachable', detail: String(err) }))
        }
      })
    },
  }
}

export default defineConfig({
  plugins: [svelte(), api_proxy()],
  server: { port: 5173 },
  build: {
    // Dictionary entries run to 237 KB of HTML; keep the runtime lean.
    target: 'es2022',
  },
  test: {
    environment: 'jsdom',
    include: ['src/**/*.test.ts'],
  },
})
