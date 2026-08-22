const SCHRONU_CONFIG = {
  sheetNames: ['実ログ', '優先度低い順'],
  taskIdCol: 2,
  syncCols: [12, 14, 16, 18],
  dataStartRow: 3,
  timeFormatRanges: ['L3:M500', 'O3:P500'],
};

function onOpen(e) {
  SpreadsheetApp.getUi()
    .createMenu('ユーザー関数')
    .addItem('時刻形式を再適用', 'applyTimeFormat')
    .addToUi();

  applyTimeFormat();
}

function applyTimeFormat() {
  const spreadsheet = SpreadsheetApp.getActiveSpreadsheet();
  const missingSheetNames = [];

  // 「実ログ」シートは generate_command_from_spreadsheet.sh で時刻だけでなく日付も渡せるようにするために、hh:mmには変えない
  const sheetNames = [ '優先度低い順', ];

  for (const sheetName of sheetNames) {
    const sheet = spreadsheet.getSheetByName(sheetName);

    if (!sheet) {
      missingSheetNames.push(sheetName);
      continue;
    }

    sheet.getRangeList(SCHRONU_CONFIG.timeFormatRanges).setNumberFormat('hh:mm');
  }

  if (missingSheetNames.length > 0) {
    SpreadsheetApp.getUi().alert(`シートが存在しません: ${missingSheetNames.join(', ')}`);
  }
}

function onEdit(e) {
  if (!e || !e.range || !e.source) {
    return;
  }

  const range = e.range;
  const sheet = range.getSheet();

  if (!SCHRONU_CONFIG.sheetNames.includes(sheet.getName())) {
    return;
  }

  if (!rangeTouchesDataRows_(range)) {
    return;
  }

  const lock = LockService.getDocumentLock();
  if (!lock.tryLock(1000)) {
    return;
  }

  try {
    if (isCommandOutputPaste_(range)) {
      return;
    }

    if (rangeTouchesSyncCols_(range)) {
      syncEditedManualCols_(e.source, sheet, range);
    }
  } finally {
    lock.releaseLock();
  }
}

function syncEditedManualCols_(spreadsheet, sourceSheet, editedRange) {
  const otherSheet = getOtherSheet_(spreadsheet, sourceSheet.getName());

  if (!otherSheet) {
    return;
  }

  const startRow = Math.max(editedRange.getRow(), SCHRONU_CONFIG.dataStartRow);
  const endRow = editedRange.getRow() + editedRange.getNumRows() - 1;
  const startCol = editedRange.getColumn();
  const endCol = startCol + editedRange.getNumColumns() - 1;

  for (let row = startRow; row <= endRow; row++) {
    const taskId = getTaskId_(sourceSheet, row);

    if (!taskId) {
      continue;
    }

    const targetRow = findRowByTaskId_(otherSheet, taskId);

    if (!targetRow) {
      continue;
    }

    for (const col of SCHRONU_CONFIG.syncCols) {
      if (col < startCol || endCol < col) {
        continue;
      }

      const value = sourceSheet.getRange(row, col).getValue();
      otherSheet.getRange(targetRow, col).setValue(value);
    }
  }
}

function findRowByTaskId_(sheet, taskId) {
  const lastRow = sheet.getLastRow();

  if (lastRow < SCHRONU_CONFIG.dataStartRow) {
    return null;
  }

  const values = sheet
    .getRange(SCHRONU_CONFIG.dataStartRow, SCHRONU_CONFIG.taskIdCol, lastRow - SCHRONU_CONFIG.dataStartRow + 1, 1)
    .getValues();

  for (let i = 0; i < values.length; i++) {
    if (normalizeTaskId_(values[i][0]) === taskId) {
      return SCHRONU_CONFIG.dataStartRow + i;
    }
  }

  return null;
}

function getOtherSheet_(spreadsheet, sheetName) {
  const otherSheetName = SCHRONU_CONFIG.sheetNames.find((name) => name !== sheetName);

  if (!otherSheetName) {
    return null;
  }

  return spreadsheet.getSheetByName(otherSheetName);
}

function getTaskId_(sheet, row) {
  return normalizeTaskId_(sheet.getRange(row, SCHRONU_CONFIG.taskIdCol).getValue());
}

function normalizeTaskId_(value) {
  return String(value || '').trim();
}

function isCommandOutputPaste_(range) {
  const startCol = range.getColumn();
  const endCol = startCol + range.getNumColumns() - 1;

  return range.getNumRows() > 1
    && startCol <= SCHRONU_CONFIG.taskIdCol
    && SCHRONU_CONFIG.taskIdCol <= endCol;
}

function rangeTouchesDataRows_(range) {
  const endRow = range.getRow() + range.getNumRows() - 1;
  return endRow >= SCHRONU_CONFIG.dataStartRow;
}

function rangeTouchesSyncCols_(range) {
  const startCol = range.getColumn();
  const endCol = startCol + range.getNumColumns() - 1;

  return SCHRONU_CONFIG.syncCols.some((col) => startCol <= col && col <= endCol);
}
