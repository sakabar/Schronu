function onOpen(e) {
  SpreadsheetApp.getUi()
    .createMenu('ユーザー関数')
    .addItem('時刻形式を再適用', 'applyTimeFormat')
    .addToUi();

  applyTimeFormat();
}

function applyTimeFormat() {
  const sheetName = '実ログ';
  const spreadsheet = SpreadsheetApp.getActiveSpreadsheet();
  const sheet = spreadsheet.getSheetByName(sheetName);

  if (!sheet) {
    SpreadsheetApp.getUi().alert(`シートが存在しません: ${sheetName}`);
    return;
  }

  sheet.getRangeList(['K3:K500', 'M3:M500']).setNumberFormat('hh:mm');
}