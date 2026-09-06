import assert from 'node:assert/strict';
import test from 'node:test';

import { COL, loadAppsScript, taskRow } from './test_support/fake_spreadsheet.mjs';

const TASK_ID = '11111111-1111-1111-1111-111111111111';
const OTHER_TASK_ID = '22222222-2222-2222-2222-222222222222';

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

test('数値0として読み取られたA列indも先頭segmentとして同期する', () => {
  const appsScript = loadAppsScript({
    '実ログ': [taskRow(0, TASK_ID)],
    '優先度低い順': [taskRow(0, TASK_ID)],
  });

  appsScript.edit('実ログ', 3, COL.startTime, '12:34');

  assert.deepEqual(appsScript.writes, [
    { sheet: '優先度低い順', row: 3, column: COL.startTime, value: '12:34' },
  ]);
  assert.deepEqual(appsScript.toasts, []);
});

for (const [name, column, value] of [
  ['N列', COL.finishFlag, 'F'],
  ['R列', COL.deferCommand, 'W'],
]) {
  test(`${name}は両sheetの同じtask全segmentへ同期する`, () => {
    const appsScript = loadAppsScript({
      '実ログ': [
        taskRow('0000', TASK_ID),
        taskRow('0001', TASK_ID),
        taskRow('0002', OTHER_TASK_ID),
      ],
      '優先度低い順': [
        taskRow('0001', TASK_ID),
        taskRow('0002', OTHER_TASK_ID),
        taskRow('0000', TASK_ID),
      ],
    });

    appsScript.edit('実ログ', 4, column, value);

    assert.equal(appsScript.sheet('実ログ').cell(3, column), value);
    assert.equal(appsScript.sheet('実ログ').cell(4, column), value);
    assert.equal(appsScript.sheet('実ログ').cell(5, column), '');
    assert.equal(appsScript.sheet('優先度低い順').cell(3, column), value);
    assert.equal(appsScript.sheet('優先度低い順').cell(4, column), '');
    assert.equal(appsScript.sheet('優先度低い順').cell(5, column), value);
    assert.equal(appsScript.writes.length, 3);
  });
}

test('A列が欠落した編集行は書き込まず診断する', () => {
  const appsScript = loadAppsScript({
    '実ログ': [taskRow('', TASK_ID)],
    '優先度低い順': [taskRow('0000', TASK_ID)],
  });

  appsScript.edit('実ログ', 3, COL.startTime, '12:34');

  assert.deepEqual(appsScript.writes, []);
  assert.equal(appsScript.toasts.length, 1);
  assert.match(appsScript.toasts[0].message, /実ログ.*3.*A列/);
});

test('B列が欠落した編集行は書き込まず診断する', () => {
  const appsScript = loadAppsScript({
    '実ログ': [taskRow('0000', '')],
    '優先度低い順': [taskRow('0000', TASK_ID)],
  });

  appsScript.edit('実ログ', 3, COL.startTime, '12:34');

  assert.deepEqual(appsScript.writes, []);
  assert.equal(appsScript.toasts.length, 1);
  assert.match(appsScript.toasts[0].message, /実ログ.*3.*B列/);
});

test('相手sheetに対応segmentがなければ書き込まず診断する', () => {
  const appsScript = loadAppsScript({
    '実ログ': [taskRow('0001', TASK_ID)],
    '優先度低い順': [taskRow('0000', TASK_ID)],
  });

  appsScript.edit('実ログ', 3, COL.startTime, '12:34');

  assert.deepEqual(appsScript.writes, []);
  assert.equal(appsScript.toasts.length, 1);
  assert.match(appsScript.toasts[0].message, /優先度低い順.*0001.*11111111/);
});

for (const duplicateSheet of ['実ログ', '優先度低い順']) {
  test(`${duplicateSheet}のA+B重複は先頭行へ書き込まず診断する`, () => {
    const duplicateRows = [taskRow('0000', TASK_ID), taskRow('0000', TASK_ID)];
    const appsScript = loadAppsScript({
      '実ログ': duplicateSheet === '実ログ' ? duplicateRows : [taskRow('0000', TASK_ID)],
      '優先度低い順': duplicateSheet === '優先度低い順'
        ? duplicateRows
        : [taskRow('0000', TASK_ID)],
    });

    appsScript.edit('実ログ', 3, COL.startTime, '12:34');

    assert.deepEqual(appsScript.writes, []);
    assert.equal(appsScript.toasts.length, 1);
    assert.match(appsScript.toasts[0].message, new RegExp(`${duplicateSheet}.*0000.*重複`));
  });
}

for (const [name, column, values] of [
  ['N列', COL.finishFlag, [['F'], ['']]],
  ['R列', COL.deferCommand, [['W'], ['d']]],
]) {
  test(`同一taskの${name}へ異なる値を一括入力した場合は同期を拒否する`, () => {
    const normalRows = [taskRow('0000', TASK_ID), taskRow('0001', TASK_ID)];
    const appsScript = loadAppsScript({
      '実ログ': normalRows,
      '優先度低い順': normalRows,
    });

    appsScript.editRange('実ログ', 3, column, values);

    assert.deepEqual(appsScript.writes, []);
    assert.equal(appsScript.toasts.length, 1);
    assert.match(appsScript.toasts[0].message, new RegExp(`${name}.*競合`));
  });
}

test('複数行編集は全identityを確認してから書き込みを始める', () => {
  const appsScript = loadAppsScript({
    '実ログ': [taskRow('0000', TASK_ID), taskRow('0001', TASK_ID)],
    '優先度低い順': [taskRow('0000', TASK_ID)],
  });

  appsScript.editRange('実ログ', 3, COL.startTime, [['12:34'], ['13:45']]);

  assert.deepEqual(appsScript.writes, []);
  assert.equal(appsScript.toasts.length, 1);
  assert.match(appsScript.toasts[0].message, /0001.*対応/);
});

test('N列は同一taskの兄弟segmentが相手sheetになければ全書き込みを拒否する', () => {
  const appsScript = loadAppsScript({
    '実ログ': [taskRow('0000', TASK_ID), taskRow('0001', TASK_ID)],
    '優先度低い順': [taskRow('0000', TASK_ID)],
  });

  appsScript.edit('実ログ', 3, COL.finishFlag, 'F');

  assert.deepEqual(appsScript.writes, []);
  assert.equal(appsScript.toasts.length, 1);
  assert.match(appsScript.toasts[0].message, /優先度低い順.*0001.*対応/);
});

for (const duplicateSheet of ['実ログ', '優先度低い順']) {
  test(`R列は${duplicateSheet}の兄弟segment複合key重複を拒否する`, () => {
    const normalRows = [taskRow('0000', TASK_ID), taskRow('0001', TASK_ID)];
    const duplicateRows = [
      taskRow('0000', TASK_ID),
      taskRow('0001', TASK_ID),
      taskRow('0001', TASK_ID),
    ];
    const appsScript = loadAppsScript({
      '実ログ': duplicateSheet === '実ログ' ? duplicateRows : normalRows,
      '優先度低い順': duplicateSheet === '優先度低い順' ? duplicateRows : normalRows,
    });

    appsScript.edit('実ログ', 3, COL.deferCommand, 'W');

    assert.deepEqual(appsScript.writes, []);
    assert.equal(appsScript.toasts.length, 1);
    assert.match(appsScript.toasts[0].message, new RegExp(`${duplicateSheet}.*0001.*重複`));
  });
}
