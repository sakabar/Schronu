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
  const lockState = {
    acquireCalls: 0,
    releaseCalls: 0,
  };
  const context = vm.createContext({
    console: { log() {} },
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
  return { context, lockState };
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

test('対象外sheetの編集は同期処理を開始しない', () => {
  const { context, lockState } = loadScript();
  const sheet = new MockSheet('その他', [[3, 12, 'value']]);

  context.onEdit({
    source: new MockSpreadsheet([sheet]),
    range: sheet.getRange(3, 12),
  });

  assert.equal(lockState.acquireCalls, 0);
});

test('header行だけの編集は同期処理を開始しない', () => {
  const { context, lockState } = loadScript();
  const { sourceSheet, spreadsheet } = synchronizationFixture();

  context.onEdit({
    source: spreadsheet,
    range: sourceSheet.getRange(2, 12),
  });

  assert.equal(lockState.acquireCalls, 0);
});

test('同期対象外列の編集は相手sheetを変更しない', () => {
  const { context, lockState } = loadScript();
  const { sourceSheet, targetSheet, spreadsheet } = synchronizationFixture();

  context.onEdit({
    source: spreadsheet,
    range: sourceSheet.getRange(5, 13),
  });

  assert.equal(targetSheet.valueAt(4, 12), 'target-value');
  assert.equal(lockState.acquireCalls, 1);
  assert.equal(lockState.releaseCalls, 1);
});

test('B列を含む複数行pasteは相手sheetを変更しない', () => {
  const { context, lockState } = loadScript();
  const { sourceSheet, targetSheet, spreadsheet } = synchronizationFixture();

  context.onEdit({
    source: spreadsheet,
    range: sourceSheet.getRange(3, 2, 2, 11),
  });

  assert.equal(targetSheet.valueAt(4, 12), 'target-value');
  assert.equal(lockState.acquireCalls, 1);
  assert.equal(lockState.releaseCalls, 1);
});
