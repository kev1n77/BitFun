import { strict as assert } from 'node:assert';
import test from 'node:test';
import { releasePlan } from './configure.mjs';

test('both real versions use the same isolated notification channel', () => {
  const receiver = releasePlan('kev1n77/BitFun', 'updater-lab-20260910', '0.2.19');
  const publisher = releasePlan('kev1n77/BitFun', 'updater-lab-20260910', '0.2.20');
  assert.equal(receiver.endpoint, publisher.endpoint);
  assert.notEqual(receiver.endpoint, receiver.controlEndpoint);
  assert.equal(publisher.downloadPage, 'https://github.com/kev1n77/BitFun/releases/tag/updater-lab-20260910-0.2.20');
  assert.ok(!receiver.endpoint.includes('/releases/latest/'));
  assert.ok(!receiver.endpoint.includes('openbitfun.com'));
});

test('publishing cannot target the official repository, real tags, or a new product version', () => {
  assert.throws(() => releasePlan('GCWing/OpenBitFun', 'updater-lab-20260910', '0.2.19'));
  assert.throws(() => releasePlan('kev1n77/BitFun', 'v0.2.19', '0.2.19'));
  assert.throws(() => releasePlan('kev1n77/BitFun', 'updater-lab-20260910', '1.0.0'));
});
