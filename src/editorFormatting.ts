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
export interface FormattingResult {
    content: string;
    selectionStart: number;
    selectionEnd: number;
}
export function formatHeading(content: string, cursorPosition: number): FormattingResult | null {
    const lineStart = content.lastIndexOf("\n", cursorPosition - 1) + 1;
    let lineEnd = content.indexOf("\n", cursorPosition);
    if (lineEnd === -1) {
        lineEnd = content.length;
    }
    const line = content.slice(lineStart, lineEnd);
    const headingMatch = line.match(/^(#{1,6})\s+/);
    let newLine: string;
    if (!headingMatch) {
        newLine = `# ${line}`;
    }
    else {
        const currentLevel = headingMatch[1].length;
        if (currentLevel < 6) {
            newLine = `${"#".repeat(currentLevel + 1)}${line.slice(headingMatch[1].length)}`;
        }
        else {
            newLine = line.replace(/^#{6}\s+/, "");
        }
    }
    const updatedContent = content.slice(0, lineStart) +
        newLine +
        content.slice(lineEnd);
    const position = cursorPosition + (newLine.length - line.length);
    return { content: updatedContent, selectionStart: position, selectionEnd: position };
}
export function formatBulletedList(content: string, selectionStart: number, selectionEnd: number): FormattingResult | null {
    const blockStart = content.lastIndexOf("\n", selectionStart - 1) + 1;
    let blockEnd = content.indexOf("\n", selectionEnd);
    if (blockEnd === -1) {
        blockEnd = content.length;
    }
    const block = content.slice(blockStart, blockEnd);
    const lines = block.split("\n");
    const nonEmptyLines = lines.filter((line) => line.trim() !== "");
    if (nonEmptyLines.length === 0)
        return null;
    const allBulleted = nonEmptyLines.every((line) => /^\s*-\s+/.test(line));
    const newLines = lines.map((line) => {
        if (line.trim() === "")
            return line;
        if (allBulleted) {
            return line.replace(/^(\s*)-\s+/, "$1");
        }
        return `- ${line}`;
    });
    const newBlock = newLines.join("\n");
    const updatedContent = content.slice(0, blockStart) +
        newBlock +
        content.slice(blockEnd);
    const difference = newBlock.length - block.length;
    return { content: updatedContent, selectionStart: Math.max(0, selectionStart + difference), selectionEnd: Math.max(0, selectionEnd + difference) };
}
export function formatNumberedList(content: string, selectionStart: number, selectionEnd: number): FormattingResult | null {
    const blockStart = content.lastIndexOf("\n", selectionStart - 1) + 1;
    let blockEnd = content.indexOf("\n", selectionEnd);
    if (blockEnd === -1) {
        blockEnd = content.length;
    }
    const block = content.slice(blockStart, blockEnd);
    const lines = block.split("\n");
    const nonEmptyLines = lines.filter((line) => line.trim() !== "");
    if (nonEmptyLines.length === 0)
        return null;
    const allNumbered = nonEmptyLines.every((line) => /^\s*\d+\.\s+/.test(line));
    let number = 1;
    const newLines = lines.map((line) => {
        if (line.trim() === "")
            return line;
        if (allNumbered) {
            return line.replace(/^(\s*)\d+\.\s+/, "$1");
        }
        const newLine = `${number}. ${line}`;
        number++;
        return newLine;
    });
    const newBlock = newLines.join("\n");
    const updatedContent = content.slice(0, blockStart) +
        newBlock +
        content.slice(blockEnd);
    const difference = newBlock.length - block.length;
    return { content: updatedContent, selectionStart: Math.max(0, selectionStart + difference), selectionEnd: Math.max(0, selectionEnd + difference) };
}
export function formatChecklist(content: string, selectionStart: number, selectionEnd: number): FormattingResult | null {
    const blockStart = content.lastIndexOf("\n", selectionStart - 1) + 1;
    let blockEnd = content.indexOf("\n", selectionEnd);
    if (blockEnd === -1) {
        blockEnd = content.length;
    }
    const block = content.slice(blockStart, blockEnd);
    const lines = block.split("\n");
    const nonEmptyLines = lines.filter((line) => line.trim() !== "");
    if (nonEmptyLines.length === 0)
        return null;
    const checklistPattern = /^\s*-\s+\[[ xX]\]\s+/;
    const allChecklist = nonEmptyLines.every((line) => checklistPattern.test(line));
    const newLines = lines.map((line) => {
        if (line.trim() === "")
            return line;
        if (allChecklist) {
            return line.replace(/^(\s*)-\s+\[[ xX]\]\s+/, "$1");
        }
        return `- [ ] ${line}`;
    });
    const newBlock = newLines.join("\n");
    const updatedContent = content.slice(0, blockStart) +
        newBlock +
        content.slice(blockEnd);
    return { content: updatedContent, selectionStart: blockStart, selectionEnd: blockStart + newBlock.length };
}
/** Refuse stale selections: even edits outside the selection invalidate its offsets. */
export function applyPendingLink(content: string, link: {
    content: string;
    start: number;
    end: number;
    text: string;
    url: string;
}): string | null {
    if (content !== link.content || content.slice(link.start, link.end) !== link.text || !link.url.trim())
        return null;
    return content.slice(0, link.start) + `[${link.text}](${link.url.trim()})` + content.slice(link.end);
}
