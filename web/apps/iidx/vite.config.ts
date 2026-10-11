import { defineConfig } from 'vite';
import { fileURLToPath } from 'node:url';
import { readFile } from 'node:fs/promises';
import { resolve } from 'node:path';
const target = process.env.BILIMANI_API_URL ?? 'http://127.0.0.1:32133';
const root = process.env.BILIMANI_WEB_ROOT ?? fileURLToPath(new URL('../../../dist/web/', import.meta.url));
const proxy = (rewrite?: (path: string) => string) => ({
  target, changeOrigin: true, rewrite,
  configure: (server: { on: (event: 'proxyReq', cb: (req: { setHeader: (key: string, value: string) => void }) => void) => void }) => {
    server.on('proxyReq', req => req.setHeader('Origin', target));
  },
});
export default defineConfig({
  base: './', publicDir: false,
  plugins: [{ name: 'runtime-page-config', configureServer(server) {
    server.middlewares.use(async (req, res, next) => {
      const match = /^\/(card|mecha)\/config.json(?:\?.*)?$/.exec(req.url ?? '');
      if (!match) { next(); return; }
      try {
        const bytes = await readFile(resolve(root, 'iidx', match[1], 'config.json'));
        res.setHeader('Content-Type', 'application/json'); res.setHeader('Cache-Control', 'no-store'); res.end(bytes);
      } catch { res.statusCode = 404; res.end('Cannot read page config.json; run npm run build to prepare the page files'); }
    });
  } }],
  server: { proxy: {
    '/api': proxy(),
    '/card/_proxy': proxy(path => `/iidx${path}`),
    '/mecha/_proxy': proxy(path => `/iidx${path}`),
  } },
});
