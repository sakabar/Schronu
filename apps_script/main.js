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

  const shouldMeasureSync = !isCommandOutputPaste_(range) && rangeTouchesSyncCols_(range);
  const syncBenchmark = shouldMeasureSync
    ? createSyncBenchmark_(sheet.getName(), getOtherSheetName_(sheet.getName()))
    : null;
  const lock = LockService.getDocumentLock();
  const lockStartedAt = Date.now();
  const lockAcquired = lock.tryLock(1000);

  if (syncBenchmark) {
    syncBenchmark.lockWaitMs = elapsedMillisecondsSince_(lockStartedAt);
  }

  if (!lockAcquired) {
    if (syncBenchmark) {
      syncBenchmark.outcome = 'lock_unavailable';
      logSyncBenchmark_(syncBenchmark);
    }
    return;
  }

  let syncCompleted = false;

  try {
    if (isCommandOutputPaste_(range)) {
      return;
    }

    if (rangeTouchesSyncCols_(range)) {
      syncEditedManualCols_(e.source, sheet, range, syncBenchmark);
      syncCompleted = true;
    }
  } finally {
    lock.releaseLock();
    if (syncBenchmark && syncCompleted) {
      logSyncBenchmark_(syncBenchmark);
    }
  }
}

function syncEditedManualCols_(spreadsheet, sourceSheet, editedRange, syncBenchmark) {
  const otherSheet = getOtherSheet_(spreadsheet, sourceSheet.getName());

  if (!otherSheet) {
    syncBenchmark.outcome = 'target_sheet_missing';
    return;
  }

  const startRow = Math.max(editedRange.getRow(), SCHRONU_CONFIG.dataStartRow);
  const endRow = editedRange.getRow() + editedRange.getNumRows() - 1;
  const startCol = editedRange.getColumn();
  const endCol = startCol + editedRange.getNumColumns() - 1;

  for (let row = startRow; row <= endRow; row++) {
    const taskId = measureSyncStage_(syncBenchmark, 'sourceTaskIdReadMs', () =>
      getTaskId_(sourceSheet, row));

    if (!taskId) {
      if (syncBenchmark.outcome !== 'synced') {
        syncBenchmark.outcome = 'source_task_id_empty';
      }
      continue;
    }

    const targetRow = findRowByTaskId_(otherSheet, taskId, syncBenchmark);

    if (!targetRow) {
      if (syncBenchmark.outcome !== 'synced') {
        syncBenchmark.outcome = 'target_task_not_found';
      }
      continue;
    }

    for (const col of SCHRONU_CONFIG.syncCols) {
      if (col < startCol || endCol < col) {
        continue;
      }

      const value = measureSyncStage_(syncBenchmark, 'sourceValueReadMs', () =>
        sourceSheet.getRange(row, col).getValue());
      measureSyncStage_(syncBenchmark, 'targetValueWriteCallMs', () =>
        otherSheet.getRange(targetRow, col).setValue(value));
      syncBenchmark.outcome = 'synced';
    }
  }
}

function findRowByTaskId_(sheet, taskId, syncBenchmark) {
  const lastRow = measureSyncStage_(syncBenchmark, 'targetLastRowReadMs', () =>
    sheet.getLastRow());

  if (lastRow < SCHRONU_CONFIG.dataStartRow) {
    return null;
  }

  const values = measureSyncStage_(syncBenchmark, 'targetIdReadMs', () =>
    sheet
      .getRange(SCHRONU_CONFIG.dataStartRow, SCHRONU_CONFIG.taskIdCol, lastRow - SCHRONU_CONFIG.dataStartRow + 1, 1)
      .getValues());
  syncBenchmark.rowsScanned += values.length;

  return measureSyncStage_(syncBenchmark, 'targetIdSearchMs', () => {
    for (let i = 0; i < values.length; i++) {
      if (normalizeTaskId_(values[i][0]) === taskId) {
        return SCHRONU_CONFIG.dataStartRow + i;
      }
    }

    return null;
  });
}

function getOtherSheet_(spreadsheet, sheetName) {
  const otherSheetName = getOtherSheetName_(sheetName);

  if (!otherSheetName) {
    return null;
  }

  return spreadsheet.getSheetByName(otherSheetName);
}

function getOtherSheetName_(sheetName) {
  return SCHRONU_CONFIG.sheetNames.find((name) => name !== sheetName) || '';
}

function getTaskId_(sheet, row) {
  return normalizeTaskId_(sheet.getRange(row, SCHRONU_CONFIG.taskIdCol).getValue());
}

function normalizeTaskId_(value) {
  return String(value || '').trim();
}

function createSyncBenchmark_(sourceSheet, targetSheet) {
  return {
    event: 'td_014_single_cell_sync',
    outcome: 'target_task_not_found',
    sourceSheet,
    targetSheet,
    rowsScanned: 0,
    lockWaitMs: 0,
    sourceTaskIdReadMs: 0,
    targetLastRowReadMs: 0,
    targetIdReadMs: 0,
    targetIdSearchMs: 0,
    sourceValueReadMs: 0,
    targetValueWriteCallMs: 0,
  };
}

function measureSyncStage_(syncBenchmark, field, operation) {
  const startedAt = Date.now();

  try {
    return operation();
  } finally {
    syncBenchmark[field] += elapsedMillisecondsSince_(startedAt);
  }
}

function elapsedMillisecondsSince_(startedAt) {
  return Math.max(0, Date.now() - startedAt);
}

function logSyncBenchmark_(syncBenchmark) {
  console.log(JSON.stringify(syncBenchmark));
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
