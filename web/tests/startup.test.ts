import test from 'node:test';
import assert from 'node:assert/strict';
import { advanceLogin, initialLogin, loadingCells, loadingCycle, loadingPattern, loadingCellLit } from '../apps/iidx/mecha/startup-motion';

test('startup waits for selection, then confirms once and fades without polling restarting it', () => {
  let state = advanceLogin(initialLogin(0), 'other', 100, true);
  assert.equal(state.stage, 'waiting');
  state = advanceLogin(state, 'select', 1000, true);
  assert.equal(state.stage, 'success');
  state = advanceLogin(state, 'select', 2000, true);
  assert.equal(state.since, 1000);
  state = advanceLogin(state, 'playing', 2400, true);
  assert.equal(state.stage, 'fading');
  state = advanceLogin(state, 'playing', 3400, true);
  assert.equal(state.stage, 'hidden');
  assert.equal(advanceLogin(state, 'other', 8000, true).stage, 'hidden', 'results and song changes cannot re-cover the game');
});

test('refresh during selection or gameplay never runs a fake login confirmation', () => {
  for (const phase of ['select', 'playing'] as const) {
    assert.equal(advanceLogin(initialLogin(0), phase, 100, true).stage, 'hidden');
  }
  let state = advanceLogin(initialLogin(0), null, 700, true);
  assert.equal(state.stage, 'waiting');
  state = advanceLogin(state, 'select', 1500, true);
  assert.equal(state.stage, 'success', 'a real connection wait still gets confirmation');
});

test('brief API gaps do not reopen the mask; a sustained game disconnect rearms startup', () => {
  let state = advanceLogin(initialLogin(0), 'select', 100, true);
  state = advanceLogin(state, null, 500, true);
  assert.equal(advanceLogin(state, null, 4000, true).stage, 'hidden');
  state = advanceLogin(state, 'other', 4200, true);
  state = advanceLogin(state, null, 5000, true);
  state = advanceLogin(state, null, 9000, true);
  assert.equal(state.stage, 'waiting');
  state = advanceLogin(state, 'select', 10000, false);
  assert.equal(state.stage, 'success');
  assert.equal(advanceLogin(state, 'select', 11400, false).stage, 'hidden', 'disabled effects skip the fade');
});

test('honeycomb sweeps fill left to right, retain random holes and clear before repeating', () => {
  let index = 0;
  const samples = [.2, .3, .7, .9, .8, .1];
  const sweep = loadingPattern(() => samples[index++ % samples.length]);
  const litAt = (time: number) => loadingCells.filter((_, i) => loadingCellLit(sweep[i], time));
  assert.equal(litAt(-1).length, 0);
  const early = litAt(600), later = litAt(2300);
  assert.ok(early.length > 0 && later.length > early.length);
  assert.ok(Math.max(...early.map(cell => cell.x)) < Math.max(...later.map(cell => cell.x)));
  assert.ok(litAt(2900).length < loadingCells.length, 'some cells stay hollow throughout the sweep');
  assert.equal(litAt(loadingCycle).length, 0, 'the old pattern is clear before a new sweep starts');
});

test('an explicit unavailable mode returns to login waiting; an ordinary song gap does not', () => {
  let state = advanceLogin(initialLogin(0), 'select', 100, true, 'SP');
  state = advanceLogin(state, 'other', 1000, true, 'SP');
  assert.equal(advanceLogin(state, 'other', 5000, true, 'SP').stage, 'hidden');
  state = advanceLogin(state, 'other', 6000, true, null);
  assert.equal(advanceLogin(state, 'other', 6500, true, null).stage, 'hidden');
  assert.equal(advanceLogin(state, 'other', 7200, true, null).stage, 'waiting');
});
