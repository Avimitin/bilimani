import { build } from 'vite';
import { copyFile, readFile, writeFile } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
const preview = process.argv.includes('--preview');
const output = preview ? 'preview' : 'web';
for (const style of ['card', 'mecha']) {
  const configPath = new URL(`../../../dist/${output}/iidx/${style}/config.json`, import.meta.url);
  const existing = await readFile(configPath).catch(error => {
    if (error.code === 'ENOENT') return readFile(new URL('./config.json', import.meta.url));
    throw error;
  });
  let config = existing;
  if (preview) {
    const defaults = JSON.parse(existing);
    config = JSON.stringify({ ...defaults, platform: { ...defaults.platform, enabled: false } }, null, 2) + '\n';
  }
  try {
    await build({
      configFile: false,
      root: fileURLToPath(new URL(`./${style}/`, import.meta.url)),
      base: './', publicDir: false,
      resolve: { alias: preview ? { './chat-source': fileURLToPath(new URL('../../preview/source.ts', import.meta.url)) } : {} },
      build: { outDir: fileURLToPath(new URL(`../../../dist/${output}/iidx/${style}/`, import.meta.url)), emptyOutDir: true },
    });
  } finally {
    await writeFile(configPath, config);
  }
}
await copyFile(new URL('../../index.html', import.meta.url), new URL(`../../../dist/${output}/index.html`, import.meta.url));
