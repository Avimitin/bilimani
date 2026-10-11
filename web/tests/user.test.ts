import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { emptyUser, parseUser, danLabel } from '../apps/iidx/user';
import { parseConfig } from '../apps/iidx/config';

const player = () => ({ side: 2, dj_name: 'PLAYER', sp: { dan: 'kaiden', dj_points: 12345 }, dp: { dan: null, dj_points: 0 }, weekly: null });
test('player telemetry keeps missing statistics distinct from zero and rejects malformed identities', () => {
  assert.deepEqual(parseUser(emptyUser()), emptyUser());
  const user = player();
  assert.deepEqual(parseUser({ active_side: 2, players: [user] }).players[0].weekly, null);
  for (const invalid of [null, {}, { active_side: 1, players: [user] }, { active_side: 2, players: [user, user] },
    { active_side: 2, players: [{ ...user, dj_name: '' }] }, { active_side: 2, players: [{ ...user, sp: { dan: 'dan11', dj_points: 2 } }] },
    { active_side: 2, players: [{ ...user, dp: { dan: null, dj_points: -1 } }] },
    { active_side: 2, players: [{ ...user, weekly: { period: 'rolling_week', week_id: 1, date: 1, sp: null, dp: null } }] }]) {
    assert.throws(() => parseUser(invalid));
  }
  const weekly = { period: 'game_week', week_id: 1, date: 2, sp: { songs: 0, keys: 0 }, dp: null };
  assert.deepEqual(parseUser({ active_side: 2, players: [{ ...user, weekly }] }).players[0].weekly, weekly);
  assert.equal(danLabel('kyu7'), '七級');
  assert.equal(danLabel('dan1'), '初段');
  assert.equal(danLabel('dan10'), '十段');
  assert.equal(danLabel('chuden'), '中伝');
  assert.equal(danLabel(null), '未取得');
});
test('rotation defaults to 10 seconds with existing runtime configs and validates explicit values', () => {
  const config = JSON.parse(readFileSync(new URL('../apps/iidx/config.json', import.meta.url), 'utf8'));
  config.platform.enabled = false;
  assert.equal(parseConfig(config).display.infoRotationSeconds, 10);
  assert.equal(parseConfig(config).display.vfdEffects, true);
  delete config.display.vfdEffects;
  assert.equal(parseConfig(config).display.vfdEffects, true);
  config.display.vfdEffects = false;
  assert.equal(parseConfig(config).display.vfdEffects, false);
  config.display.vfdEffects = 'false';
  assert.throws(() => parseConfig(config), /vfdEffects/);
  config.display.vfdEffects = true;
  delete config.display.infoRotationSeconds;
  assert.equal(parseConfig(config).display.infoRotationSeconds, 10);
  config.display.infoRotationSeconds = 30;
  assert.equal(parseConfig(config).display.infoRotationSeconds, 30);
  for (const value of [0, -1, 3601, 1.5, '10', null]) {
    config.display.infoRotationSeconds = value;
    assert.throws(() => parseConfig(config), /infoRotationSeconds/);
  }
});
