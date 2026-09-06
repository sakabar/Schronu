const SCHRONU_CONFIG = {
  sheetNames: ['実ログ', '優先度低い順'],
  indCol: 1,
  taskIdCol: 2,
  syncCols: [12, 14, 16, 18],
  segmentSyncCols: [12, 16],
  taskSyncCols: [14, 18],
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
    spreadsheet.toast(
      `同期先シートが存在しません: ${sourceSheet.getName()}`,
      'Schronu同期エラー',
    );
    return;
  }

  const startRow = Math.max(editedRange.getRow(), SCHRONU_CONFIG.dataStartRow);
  const endRow = editedRange.getRow() + editedRange.getNumRows() - 1;
  const startCol = editedRange.getColumn();
  const endCol = startCol + editedRange.getNumColumns() - 1;
  const editedSyncCols = SCHRONU_CONFIG.syncCols.filter(
    (col) => startCol <= col && col <= endCol,
  );
  const sourceIndex = buildIdentityIndex_(sourceSheet);
  const targetIndex = buildIdentityIndex_(otherSheet);
  const errors = [];
  const writes = new Map();

  for (let row = startRow; row <= endRow; row++) {
    const sourceIdentity = sourceIndex.byRow.get(row) || { ind: '', taskId: '' };
    const { ind, taskId } = sourceIdentity;

    if (!ind) {
      errors.push(`${sourceSheet.getName()} ${row}行: A列indが空です`);
    }
    if (!taskId) {
      errors.push(`${sourceSheet.getName()} ${row}行: B列task_idが空です`);
    }
    if (!ind || !taskId) {
      continue;
    }

    const segmentKey = makeSegmentKey_(ind, taskId);
    const sourceSegmentRows = sourceIndex.bySegment.get(segmentKey) || [];
    const targetSegmentRows = targetIndex.bySegment.get(segmentKey) || [];

    if (sourceSegmentRows.length > 1) {
      errors.push(
        `${sourceSheet.getName()} A=${ind} B=${taskId}: 複合keyが重複しています`,
      );
    }
    if (targetSegmentRows.length === 0) {
      errors.push(
        `${otherSheet.getName()} A=${ind} B=${taskId}: 対応segmentがありません`,
      );
    } else if (targetSegmentRows.length > 1) {
      errors.push(
        `${otherSheet.getName()} A=${ind} B=${taskId}: 複合keyが重複しています`,
      );
    }
    if (sourceSegmentRows.length !== 1 || targetSegmentRows.length !== 1) {
      continue;
    }

    for (const col of editedSyncCols) {
      const value = sourceSheet.getRange(row, col).getValue();
      if (SCHRONU_CONFIG.taskSyncCols.includes(col)) {
        for (const [sheet, index] of [
          [sourceSheet, sourceIndex],
          [otherSheet, targetIndex],
        ]) {
          for (const taskIdentity of index.byTask.get(taskId) || []) {
            if (!taskIdentity.ind) {
              errors.push(`${sheet.getName()} ${taskIdentity.row}行: A列indが空です`);
              continue;
            }
            if (sheet === sourceSheet && taskIdentity.row === row) {
              continue;
            }
            planWrite_(writes, sheet, taskIdentity.row, col, value);
          }
        }
      } else {
        planWrite_(writes, otherSheet, targetSegmentRows[0].row, col, value);
      }
    }
  }

  if (errors.length > 0) {
    spreadsheet.toast([...new Set(errors)].join('\n'), 'Schronu同期エラー');
    return;
  }

  for (const write of writes.values()) {
    write.sheet.getRange(write.row, write.col).setValue(write.value);
  }
}

function buildIdentityIndex_(sheet) {
  const lastRow = sheet.getLastRow();
  const index = {
    byRow: new Map(),
    bySegment: new Map(),
    byTask: new Map(),
  };

  if (lastRow < SCHRONU_CONFIG.dataStartRow) {
    return index;
  }

  const values = sheet
    .getRange(SCHRONU_CONFIG.dataStartRow, SCHRONU_CONFIG.indCol, lastRow - SCHRONU_CONFIG.dataStartRow + 1, 2)
    .getValues();

  for (let offset = 0; offset < values.length; offset++) {
    const identity = {
      row: SCHRONU_CONFIG.dataStartRow + offset,
      ind: normalizeInd_(values[offset][0]),
      taskId: normalizeTaskId_(values[offset][1]),
    };
    index.byRow.set(identity.row, identity);
    if (identity.taskId) {
      appendIndexValue_(index.byTask, identity.taskId, identity);
    }
    if (identity.ind && identity.taskId) {
      appendIndexValue_(
        index.bySegment,
        makeSegmentKey_(identity.ind, identity.taskId),
        identity,
      );
    }
  }

  return index;
}

function appendIndexValue_(index, key, value) {
  const values = index.get(key) || [];
  values.push(value);
  index.set(key, values);
}

function makeSegmentKey_(ind, taskId) {
  return `${ind}\u0000${taskId}`;
}

function planWrite_(writes, sheet, row, col, value) {
  writes.set(`${sheet.getName()}\u0000${row}\u0000${col}`, {
    sheet,
    row,
    col,
    value,
  });
}

function getOtherSheet_(spreadsheet, sheetName) {
  const otherSheetName = SCHRONU_CONFIG.sheetNames.find((name) => name !== sheetName);

  if (!otherSheetName) {
    return null;
  }

  return spreadsheet.getSheetByName(otherSheetName);
}

function normalizeInd_(value) {
  return String(value || '').trim();
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
