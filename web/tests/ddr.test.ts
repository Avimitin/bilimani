import { test } from 'node:test';
import assert from 'node:assert/strict';
import { ddrRules } from '../apps/ddr/rules.ts';
import { parseConfig } from '../apps/ddr/config.ts';
import { readFileSync } from 'node:fs';
test('DDR stream labels are optional page-local display settings', () => {
  const config = JSON.parse(readFileSync(new URL('../apps/ddr/config.json', import.meta.url), 'utf8'));
  config.platform.enabled = false;
  delete config.display.streamerName; delete config.display.streamTitle;
  assert.equal(parseConfig(config).display.streamTitle, '');
  config.display.streamerName = '  DANCE CREW  ';
  config.display.streamTitle = '今晚的舞池';
  assert.equal(parseConfig(config).display.streamerName, 'DANCE CREW');
  assert.equal(parseConfig(config).display.streamTitle, '今晚的舞池');
  config.display.streamTitle = 'invalid\nlabel';
  assert.throws(() => parseConfig(config));
});
test('DDR chart vocabulary distinguishes beginner from basic and derives mode', () => {
  const rules = ddrRules();
  assert.deepEqual(rules.parse('MAX 300 bSP', 'DP'), { query: 'MAX 300', mode: 'SP', chart: 'bSP' });
  assert.deepEqual(rules.parse('MAX 300 BSP', 'DP'), { query: 'MAX 300', mode: 'SP', chart: 'BSP' });
  assert.deepEqual(rules.parse('MAX 300 edp', 'SP'), { query: 'MAX 300', mode: 'DP', chart: 'EDP' });
  assert.deepEqual(rules.parse('PARANOiA', 'DP'), { query: 'PARANOiA', mode: 'DP', chart: undefined });
  assert.equal(rules.parse('song bDP', 'SP').chart, 'BDP');
});

test('DDR room metadata endpoint uses a configured proxy or a clean HTTPS URL', () => {
  const config = JSON.parse(readFileSync(new URL('../apps/ddr/config.json', import.meta.url), 'utf8'));
  config.platform.enabled = false;
  assert.equal(parseConfig(config).platform.roomInfoUrl, './_proxy/bilibili');
  for (const endpoint of ['./_proxy/missing', 'http://example.test', 'https://example.test/?authCode=private', 123]) {
    config.platform.roomInfoUrl = endpoint;
    assert.throws(() => parseConfig(config));
  }
  delete config.platform.roomInfoUrl;
  assert.equal(parseConfig(config).platform.roomInfoUrl, undefined);
});
