const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const test = require('node:test');
const vm = require('node:vm');

const SCRIPT_PATH = path.join(__dirname, 'main.js');
const SCRIPT_SOURCE = fs.readFileSync(SCRIPT_PATH, 'utf8');

class MockRange {
  constructor(sheet, row, column, numRows = 1, numColumns = 1) {
    this.sheet = sheet;
    this.row = row;
    this.column = column;
    this.numRows = numRows;
    this.numColumns = numColumns;
  }

  getSheet() {
    return this.sheet;
  }

  getRow() {
    return this.row;
  }

  getColumn() {
    return this.column;
  }

  getNumRows() {
    return this.numRows;
  }

  getNumColumns() {
    return this.numColumns;
  }

  getValue() {
    return this.sheet.valueAt(this.row, this.column);
  }

  getValues() {
    return Array.from({ length: this.numRows }, (_, rowOffset) =>
      Array.from({ length: this.numColumns }, (_, columnOffset) =>
        this.sheet.valueAt(this.row + rowOffset, this.column + columnOffset),
      ),
    );
  }

  setValue(value) {
    this.sheet.setValue(this.row, this.column, value);
    return this;
  }
}

class MockSheet {
  constructor(name, cells = []) {
    this.name = name;
    this.cells = new Map();
    for (const [row, column, value] of cells) {
      this.setValue(row, column, value);
    }
  }

  getName() {
    return this.name;
  }

  getRange(row, column, numRows = 1, numColumns = 1) {
    return new MockRange(this, row, column, numRows, numColumns);
  }

  getLastRow() {
    return Math.max(0, ...Array.from(this.cells.keys(), (key) => Number(key.split(':')[0])));
  }

  valueAt(row, column) {
    return this.cells.get(`${row}:${column}`) ?? '';
  }

  setValue(row, column, value) {
    this.cells.set(`${row}:${column}`, value);
  }
}

class MockSpreadsheet {
  constructor(sheets) {
    this.sheets = new Map(sheets.map((sheet) => [sheet.getName(), sheet]));
  }

  getSheetByName(name) {
    return this.sheets.get(name) ?? null;
  }
}

function loadScript({ lockAvailable = true } = {}) {
  const logs = [];
  const lockState = {
    acquireCalls: 0,
    releaseCalls: 0,
  };
  const context = vm.createContext({
    console: {
      log(message) {
        logs.push(message);
      },
    },
    LockService: {
      getDocumentLock() {
        return {
          tryLock() {
            lockState.acquireCalls += 1;
            return lockAvailable;
          },
          releaseLock() {
            lockState.releaseCalls += 1;
          },
        };
      },
    },
  });
  vm.runInContext(SCRIPT_SOURCE, context, { filename: SCRIPT_PATH });
  return { context, lockState, logs };
}

function synchronizationFixture() {
  const sourceSheet = new MockSheet('実ログ', [
    [5, 2, 'task-2'],
    [5, 12, 'source-value'],
  ]);
  const targetSheet = new MockSheet('優先度低い順', [
    [3, 2, 'task-1'],
    [4, 2, 'task-2'],
    [4, 12, 'target-value'],
  ]);
  return {
    sourceSheet,
    targetSheet,
    spreadsheet: new MockSpreadsheet([sourceSheet, targetSheet]),
  };
}

test('1セル編集は同じtask IDの同期列へ値を反映する', () => {
  const { context, lockState } = loadScript();
  const { sourceSheet, targetSheet, spreadsheet } = synchronizationFixture();

  context.onEdit({
    source: spreadsheet,
    range: sourceSheet.getRange(5, 12),
  });

  assert.equal(targetSheet.valueAt(4, 12), 'source-value');
  assert.equal(lockState.acquireCalls, 1);
  assert.equal(lockState.releaseCalls, 1);
});

test('1セル同期は機密値を含まない区間計測を1件記録する', () => {
  const { context, logs } = loadScript();
  const { sourceSheet, spreadsheet } = synchronizationFixture();

  context.onEdit({
    source: spreadsheet,
    range: sourceSheet.getRange(5, 12),
  });

  assert.equal(logs.length, 1);
  const metric = JSON.parse(logs[0]);
  assert.deepEqual(
    Object.keys(metric).sort(),
    [
      'event',
      'lockWaitMs',
      'outcome',
      'rowsScanned',
      'sourceSheet',
      'sourceTaskIdReadMs',
      'sourceValueReadMs',
      'targetIdReadMs',
      'targetIdSearchMs',
      'targetLastRowReadMs',
      'targetSheet',
      'targetValueWriteCallMs',
    ],
  );
  assert.equal(metric.event, 'td_014_single_cell_sync');
  assert.equal(metric.outcome, 'synced');
  assert.equal(metric.sourceSheet, '実ログ');
  assert.equal(metric.targetSheet, '優先度低い順');
  assert.equal(metric.rowsScanned, 2);
  for (const field of [
    'lockWaitMs',
    'sourceTaskIdReadMs',
    'targetLastRowReadMs',
    'targetIdReadMs',
    'targetIdSearchMs',
    'sourceValueReadMs',
    'targetValueWriteCallMs',
  ]) {
    assert.equal(Number.isInteger(metric[field]), true, `${field} must be an integer`);
    assert.equal(metric[field] >= 0, true, `${field} must be non-negative`);
  }
  assert.equal(logs[0].includes('task-2'), false);
  assert.equal(logs[0].includes('source-value'), false);
  assert.equal(logs[0].includes('target-value'), false);
});

test('lock失敗は同期せず計測結果へ記録する', () => {
  const { context, lockState, logs } = loadScript({ lockAvailable: false });
  const { sourceSheet, targetSheet, spreadsheet } = synchronizationFixture();

  context.onEdit({
    source: spreadsheet,
    range: sourceSheet.getRange(5, 12),
  });

  assert.equal(targetSheet.valueAt(4, 12), 'target-value');
  assert.equal(lockState.releaseCalls, 0);
  assert.equal(JSON.parse(logs[0]).outcome, 'lock_unavailable');
});

test('空task IDは同期せず計測結果へ記録する', () => {
  const { context, logs } = loadScript();
  const { sourceSheet, targetSheet, spreadsheet } = synchronizationFixture();
  sourceSheet.setValue(5, 2, '');

  context.onEdit({
    source: spreadsheet,
    range: sourceSheet.getRange(5, 12),
  });

  assert.equal(targetSheet.valueAt(4, 12), 'target-value');
  assert.equal(JSON.parse(logs[0]).outcome, 'source_task_id_empty');
});

test('同期先にtask IDがなければ計測結果へ記録する', () => {
  const { context, logs } = loadScript();
  const { sourceSheet, targetSheet, spreadsheet } = synchronizationFixture();
  targetSheet.setValue(4, 2, 'different-task');

  context.onEdit({
    source: spreadsheet,
    range: sourceSheet.getRange(5, 12),
  });

  assert.equal(targetSheet.valueAt(4, 12), 'target-value');
  assert.equal(JSON.parse(logs[0]).outcome, 'target_task_not_found');
});

test('相手sheetがなければ計測結果へ記録する', () => {
  const { context, logs } = loadScript();
  const { sourceSheet } = synchronizationFixture();

  context.onEdit({
    source: new MockSpreadsheet([sourceSheet]),
    range: sourceSheet.getRange(5, 12),
  });

  const metric = JSON.parse(logs[0]);
  assert.equal(metric.outcome, 'target_sheet_missing');
  assert.equal(metric.targetSheet, '優先度低い順');
});

test('Spreadsheet API例外は正常sampleへ混入させず再送出する', () => {
  const { context, lockState, logs } = loadScript();
  const { sourceSheet, targetSheet, spreadsheet } = synchronizationFixture();
  targetSheet.getLastRow = () => {
    throw new Error('target ID read failed');
  };

  assert.throws(
    () => context.onEdit({
      source: spreadsheet,
      range: sourceSheet.getRange(5, 12),
    }),
    /target ID read failed/,
  );

  assert.equal(logs.length, 0);
  assert.equal(lockState.releaseCalls, 1);
});

test('対象外sheetの編集は同期処理を開始しない', () => {
  const { context, lockState, logs } = loadScript();
  const sheet = new MockSheet('その他', [[3, 12, 'value']]);

  context.onEdit({
    source: new MockSpreadsheet([sheet]),
    range: sheet.getRange(3, 12),
  });

  assert.equal(lockState.acquireCalls, 0);
  assert.equal(logs.length, 0);
});

test('header行だけの編集は同期処理を開始しない', () => {
  const { context, lockState, logs } = loadScript();
  const { sourceSheet, spreadsheet } = synchronizationFixture();

  context.onEdit({
    source: spreadsheet,
    range: sourceSheet.getRange(2, 12),
  });

  assert.equal(lockState.acquireCalls, 0);
  assert.equal(logs.length, 0);
});

test('同期対象外列の編集は相手sheetを変更しない', () => {
  const { context, lockState, logs } = loadScript();
  const { sourceSheet, targetSheet, spreadsheet } = synchronizationFixture();

  context.onEdit({
    source: spreadsheet,
    range: sourceSheet.getRange(5, 13),
  });

  assert.equal(targetSheet.valueAt(4, 12), 'target-value');
  assert.equal(lockState.acquireCalls, 1);
  assert.equal(lockState.releaseCalls, 1);
  assert.equal(logs.length, 0);
});

test('B列を含む複数行pasteは相手sheetを変更しない', () => {
  const { context, lockState, logs } = loadScript();
  const { sourceSheet, targetSheet, spreadsheet } = synchronizationFixture();

  context.onEdit({
    source: spreadsheet,
    range: sourceSheet.getRange(3, 2, 2, 11),
  });

  assert.equal(targetSheet.valueAt(4, 12), 'target-value');
  assert.equal(lockState.acquireCalls, 1);
  assert.equal(lockState.releaseCalls, 1);
  assert.equal(logs.length, 0);
});
