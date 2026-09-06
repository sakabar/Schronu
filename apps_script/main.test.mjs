import assert from 'node:assert/strict';
import test from 'node:test';

import { COL, loadAppsScript, taskRow } from './test_support/fake_spreadsheet.mjs';

const TASK_ID = '11111111-1111-1111-1111-111111111111';

for (const [name, column] of [
  ['L列', COL.startTime],
  ['P列', COL.finishTime],
]) {
  test(`${name}はA列とB列が一致するsegmentだけへ同期する`, () => {
    const appsScript = loadAppsScript({
      '実ログ': [taskRow('0000', TASK_ID), taskRow('0001', TASK_ID)],
      '優先度低い順': [taskRow('0000', TASK_ID), taskRow('0001', TASK_ID)],
    });

    appsScript.edit('実ログ', 4, column, '12:34');

    assert.equal(appsScript.sheet('優先度低い順').cell(3, column), '');
    assert.equal(appsScript.sheet('優先度低い順').cell(4, column), '12:34');
    assert.deepEqual(appsScript.writes, [
      { sheet: '優先度低い順', row: 4, column, value: '12:34' },
    ]);
  });
}
