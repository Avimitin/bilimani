import { build } from 'vite';
import { readFile, writeFile, readdir } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
const preview = process.argv.includes('--preview');
const output = new URL(preview ? '../../../dist/ddr-preview/ddr/street/' : '../../../dist/web/ddr/street/', import.meta.url);
const config = new URL('config.json', output);
const existing = preview ? JSON.stringify({ ...JSON.parse(await readFile(new URL('./config.json', import.meta.url), 'utf8')), platform: { type: 'bilibili', enabled: false, relayUrl: './_proxy/relay' } }, null, 2) : await readFile(config).catch(error => {
  if (error.code === 'ENOENT') return readFile(new URL('./config.json', import.meta.url));
  throw error;
});
try {
  await build({ configFile: false, root: fileURLToPath(new URL('./street/', import.meta.url)),
    resolve: { alias: preview ? { './chat-source': fileURLToPath(new URL('./preview-source.ts', import.meta.url)) } : {} },
    base: './', publicDir: false, build: { outDir: fileURLToPath(output), emptyOutDir: true } });
  // Vite strips CSS comments; preserve the bundled font's license in its stylesheet.
  const source = await readFile(new URL('./street/style.css', import.meta.url), 'utf8');
  const license = source.slice(0, source.indexOf('*/') + 2);
  for (const name of await readdir(new URL('assets/', output))) {
    if (name.endsWith('.css')) {
      const path = new URL(`assets/${name}`, output);
      await writeFile(path, license + '\n' + await readFile(path, 'utf8'));
    }
  }
} finally { await writeFile(config, existing); }
