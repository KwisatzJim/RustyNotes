export interface TextInsertion {
  content: string;
  selectionStart: number;
  selectionEnd: number;
}

export function insertMarkdownTable(content: string, start: number, end: number): TextInsertion {
  const before = content.slice(0, start);
  const after = content.slice(end);
  const selected = content.slice(start, end).split("|").join("\\|").replace(/\s+/g, " ").trim();
  const leading = before.length === 0 || before.endsWith("\n\n") ? "" : before.endsWith("\n") ? "\n" : "\n\n";
  const trailing = after.length === 0 || after.startsWith("\n\n") ? "" : after.startsWith("\n") ? "\n" : "\n\n";
  const firstCell = selected || "Cell 1";
  const table = `| Column 1 | Column 2 |\n| --- | --- |\n| ${firstCell} | Cell 2 |`;
  const inserted = leading + table + trailing;
  const headerStart = before.length + leading.length + 2;

  return {
    content: before + inserted + after,
    selectionStart: headerStart,
    selectionEnd: headerStart + "Column 1".length,
  };
}
