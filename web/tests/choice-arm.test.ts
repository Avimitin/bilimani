import test from 'node:test';
import assert from 'node:assert/strict';
import { advanceArm, armDuration, armPose, armSettled, choiceOrigin, type ArmRequest, type ArmState } from '../apps/iidx/mecha/choice-arm-motion';

const opening: ArmRequest = { open: true, origin: 'right', animate: true };

test('candidate screen faces the player: SP 1P/right, SP 2P/left, DP always right', () => {
  const playing = (side: number, mode = 'SP') => ({ players: [{ side, chart: { mode } }] });
  assert.equal(choiceOrigin({ now_playing: playing(1) }), 'right');
  assert.equal(choiceOrigin({ now_playing: playing(2) }), 'left');
  assert.equal(choiceOrigin({ now_playing: playing(2, 'DP') }), 'right');
  assert.equal(choiceOrigin({ mode: 'DP', user: { active_side: 2 } }), 'right');
  assert.equal(choiceOrigin({ mode: 'SP', now_playing: playing(2, 'DP') }), 'left', 'current mode takes precedence over an old chart');
  assert.equal(choiceOrigin({ user: { active_side: 2 } }), 'left');
  assert.equal(choiceOrigin({ now_playing: playing(1), user: { active_side: 2 } }), 'right');
  assert.equal(choiceOrigin({ now_playing: { players: [null, {}] } }), 'right');
});

test('rails deploy in order, glass rolls up before the mount and rails retract', () => {
  for (let time = 0; time <= armDuration; time += 15) {
    const pose = armPose(time);
    if (pose.extensions[1] > 0) assert.equal(pose.extensions[0], 1);
    if (pose.extensions[2] > 0) assert.equal(pose.extensions[1], 1);
    if (pose.span > 0) assert.deepEqual(pose.extensions, [1, 1, 1]);
    if (pose.drop > 0) assert.equal(pose.span, 1);
    if (pose.reveal > 0) assert.equal(pose.drop, 1);
  }
  let state: ArmState = { position: armDuration, origin: 'right' };
  for (let i = 0; i < 140; i++) state = advanceArm(state, { ...opening, open: false }, 15);
  assert.equal(state.position, 0);
  assert.equal(armSettled(state, { ...opening, open: false }), true);
});

test('cancel and reopen reverse from the current pose without jumping to endpoints', () => {
  const partial = advanceArm({ position: 0, origin: 'right' }, opening, 850);
  const closing = advanceArm(partial, { ...opening, open: false }, 100);
  assert.equal(closing.position, 750);
  const reopened = advanceArm(closing, opening, 100);
  assert.deepEqual(reopened, partial);
  assert.equal(armSettled(reopened, opening), false);
});

test('side changes stow the arm completely before extending from the new edge', () => {
  const left: ArmRequest = { ...opening, origin: 'left' };
  let state: ArmState = { position: armDuration, origin: 'right' };
  state = advanceArm(state, left, 1000);
  assert.equal(state.origin, 'right');
  assert.equal(state.position, 1100);
  state = advanceArm(state, left, 1100);
  assert.equal(state.position, 0);
  assert.equal(armSettled(state, left), false);
  state = advanceArm(state, left, 120);
  assert.deepEqual(state, { position: 120, origin: 'left' });
});

test('disabling broadcast effects still presents and dismisses choices immediately', () => {
  const state: ArmState = { position: 900, origin: 'right' };
  const request: ArmRequest = { open: true, origin: 'left', animate: false };
  assert.deepEqual(advanceArm(state, request, 0), { position: armDuration, origin: 'left' });
  assert.deepEqual(advanceArm(state, { ...request, open: false }, 0), { position: 0, origin: 'left' });
});
