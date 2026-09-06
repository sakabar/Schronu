import fs from 'node:fs';
import vm from 'node:vm';

export const COL = {
  ind: 1,
  taskId: 2,
  startTime: 12,
  finishFlag: 14,
  finishTime: 16,
  deferCommand: 18,
};

export function taskRow(ind, taskId, values = {}) {
  const row = Array(19).fill('');
  row[COL.ind - 1] = ind;
  row[COL.taskId - 1] = taskId;
  for (const [column, value] of Object.entries(values)) {
    row[Number(column) - 1] = value;
  }
  return row;
}

class FakeRange {
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
    return this.getValues()[0][0];
  }

  getValues() {
    return Array.from({ length: this.numRows }, (_, rowOffset) =>
      Array.from({ length: this.numColumns }, (_, columnOffset) =>
        this.sheet.cell(this.row + rowOffset, this.column + columnOffset),
      ),
    );
  }

  setValue(value) {
    this.sheet.writeCell(this.row, this.column, value);
    return this;
  }

  setNumberFormat() {
    return this;
  }
}

class FakeSheet {
  constructor(name, dataRows, writes) {
    this.name = name;
    this.rows = [Array(19).fill(''), Array(19).fill(''), ...dataRows.map((row) => [...row])];
    this.writes = writes;
  }

  getName() {
    return this.name;
  }

  getLastRow() {
    return this.rows.length;
  }

  getRange(row, column, numRows = 1, numColumns = 1) {
    return new FakeRange(this, row, column, numRows, numColumns);
  }

  getRangeList() {
    return { setNumberFormat() {} };
  }

  cell(row, column) {
    return this.rows[row - 1]?.[column - 1] ?? '';
  }

  userEdit(row, column, value) {
    this.rows[row - 1][column - 1] = value;
  }

  writeCell(row, column, value) {
    this.rows[row - 1][column - 1] = value;
    this.writes.push({ sheet: this.name, row, column, value });
  }
}

export function loadAppsScript(sheetRows) {
  const writes = [];
  const toasts = [];
  const sheets = new Map(
    Object.entries(sheetRows).map(([name, rows]) => [name, new FakeSheet(name, rows, writes)]),
  );
  const spreadsheet = {
    getSheetByName(name) {
      return sheets.get(name) ?? null;
    },
    toast(message, title) {
      toasts.push({ message, title });
    },
  };
  const lock = {
    tryLock() {
      return true;
    },
    releaseLock() {},
  };
  const context = vm.createContext({
    LockService: { getDocumentLock: () => lock },
    SpreadsheetApp: {
      getActiveSpreadsheet: () => spreadsheet,
      getUi: () => ({
        alert() {},
        createMenu: () => ({ addItem() { return this; }, addToUi() {} }),
      }),
    },
  });
  const source = fs.readFileSync(new URL('../main.js', import.meta.url), 'utf8');
  vm.runInContext(source, context, { filename: 'apps_script/main.js' });

  return {
    sheet(name) {
      return sheets.get(name);
    },
    edit(sheetName, row, column, value, numRows = 1, numColumns = 1) {
      const sheet = sheets.get(sheetName);
      sheet.userEdit(row, column, value);
      context.event = {
        source: spreadsheet,
        range: sheet.getRange(row, column, numRows, numColumns),
      };
      vm.runInContext('onEdit(event)', context);
    },
    writes,
    toasts,
  };
}
