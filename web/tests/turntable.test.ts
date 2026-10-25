import test from 'node:test';
import assert from 'node:assert/strict';
import { turntableSpinning } from '../apps/iidx/mecha/turntable';

test('turntable spins only for an online native song decision with effects enabled', () => {
  const deciding = { phase: 'other', mode: 'SP', now_playing: { phase: 'deciding' } };
  assert.equal(turntableSpinning(deciding, true), true);
  assert.equal(turntableSpinning({ ...deciding, mode: 'DP' }, true), true);
  assert.equal(turntableSpinning(deciding, false), false);
  for (const phase of [null, 'select', 'playing']) assert.equal(turntableSpinning({ ...deciding, phase }, true), false);
  assert.equal(turntableSpinning({ ...deciding, mode: null }, true), false);
  for (const phase of ['idle', 'selecting', 'playing']) {
    assert.equal(turntableSpinning({ ...deciding, now_playing: { phase } }, true), false);
  }
  assert.equal(turntableSpinning({ phase: 'other', mode: 'SP', now_playing: null }, true), false);
});
