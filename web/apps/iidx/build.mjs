import { build } from 'vite';
import { copyFile, readFile, writeFile } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
for (const style of ['card', 'mecha']) {
  const configPath = new URL(`../../../dist/web/iidx/${style}/config.json`, import.meta.url);
  const config = await readFile(configPath).catch(error => {
    if (error.code === 'ENOENT') return readFile(new URL('./config.json', import.meta.url));
    throw error;
  });
  try {
    await build({
      configFile: false,
      root: fileURLToPath(new URL(`./${style}/`, import.meta.url)),
      base: './', publicDir: false,
      build: { outDir: fileURLToPath(new URL(`../../../dist/web/iidx/${style}/`, import.meta.url)), emptyOutDir: true },
    });
  } finally {
    await writeFile(configPath, config);
  }
}
await copyFile(new URL('../../index.html', import.meta.url), new URL('../../../dist/web/index.html', import.meta.url));
