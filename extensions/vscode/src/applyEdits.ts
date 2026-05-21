/**
 * applyEdits.ts — Routes apply_patch tool calls through VS Code WorkspaceEdit
 * for confirmable diff previews.
 *
 * Exports:
 *   applyPatchAsWorkspaceEdit(patch, options?)
 *     Parse a unified diff patch and apply it via vscode.workspace.applyEdit().
 *     By default VS Code shows a diff preview the user can confirm or reject.
 *     Set directWrite: true to skip the preview and write directly to disk.
 *
 * Patch format (mirrors the Rust-side apply_patch tool in
 * crates/tui/src/tools/apply_patch.rs):
 *
 *   --- a/<file>
 *   +++ b/<file>
 *   @@ -start,count +start,count @@
 *    context
 *   -removed
 *   +added
 *
 * Supports multi-file diffs (diff --git separators), single-file patches with
 * ---/+++ headers, and a/ b/ prefix stripping.
 */

import * as fs from 'fs';
import * as path from 'path';
import * as vscode from 'vscode';

// ── Types ───────────────────────────────────────────────────────────────────

type HunkLineKind = 'context' | 'add' | 'remove';

interface HunkLine {
  kind: HunkLineKind;
  content: string;
}

interface Hunk {
  oldStart: number;
  oldCount: number;
  newStart: number;
  newCount: number;
  lines: HunkLine[];
}

interface FilePatch {
  file: string;
  hunks: Hunk[];
  createIfMissing: boolean;
}

// ── Public API ──────────────────────────────────────────────────────────────

export interface ApplyPatchOptions {
  /**
   * If true, write directly to disk without showing the VS Code diff preview.
   * When false (default), the edit is applied via `workspace.applyEdit()`
   * which triggers the confirmable diff preview.
   */
  directWrite?: boolean;
}

/**
 * Parse a unified diff patch and apply it as a VS Code WorkspaceEdit with
 * a confirmable diff preview.
 *
 * When `directWrite` is true, writes directly to disk instead.
 *
 * Throws on parse failures or when the patch cannot be matched to the file.
 */
export async function applyPatchAsWorkspaceEdit(
  patch: string,
  options: ApplyPatchOptions = {},
): Promise<void> {
  const patches = parsePatch(patch);
  if (patches.length === 0) {
    throw new Error(
      'No valid file patches found. Ensure the patch includes ---/+++ headers.',
    );
  }

  for (const fp of patches) {
    const absPath = path.resolve(fp.file);
    const uri = vscode.Uri.file(absPath);

    // Read original content (empty string for new files).
    let originalContent: string;
    try {
      originalContent = fs.readFileSync(absPath, 'utf-8');
    } catch {
      if (fp.createIfMissing) {
        originalContent = '';
      } else {
        throw new Error(
          `File not found: ${fp.file}. Set createIfMissing=true for new files.`,
        );
      }
    }

    // Apply hunks in-memory to produce the patched content.
    const newContent = applyHunks(originalContent, fp.hunks);

    if (options.directWrite) {
      // Bypass preview — write straight to disk.
      const dir = path.dirname(absPath);
      if (!fs.existsSync(dir)) {
        fs.mkdirSync(dir, { recursive: true });
      }
      fs.writeFileSync(absPath, newContent, 'utf-8');
    } else {
      // Create a WorkspaceEdit that replaces the full file content.
      // VS Code's applyEdit automatically shows a diff preview that the
      // user can confirm (accept) or reject (cancel).
      const edit = new vscode.WorkspaceEdit();
      edit.replace(uri, fullDocumentContentRange(originalContent), newContent);

      const applied = await vscode.workspace.applyEdit(edit);
      if (!applied) {
        throw new Error(
          `VS Code rejected the edit for ${fp.file}. The diff preview may have been cancelled.`,
        );
      }
    }
  }
}

// ── Patch parser ────────────────────────────────────────────────────────────

/**
 * Parse a unified diff string into a list of `FilePatch` entries.
 *
 * Handles:
 * - Multi-file diffs separated by `diff --git <a> <b>`.
 * - Single-file patches with `---` / `+++` headers.
 * - `a/` / `b/` stripping and `/dev/null` detection.
 */
function parsePatch(patch: string): FilePatch[] {
  const patches: FilePatch[] = [];
  const lines = patch.split('\n');
  let current: FilePatch | null = null;
  let oldPath: string | null = null;
  let hunkAccum: string[] = [];

  function flushHunk() {
    if (current && hunkAccum.length > 0) {
      const hunk = parseHunkLines(hunkAccum);
      if (hunk) current.hunks.push(hunk);
      hunkAccum = [];
    }
  }

  function flushFile() {
    flushHunk();
    if (current && current.hunks.length > 0) {
      patches.push(current);
    }
    current = null;
    oldPath = null;
  }

  for (let i = 0; i < lines.length; i++) {
    const line = lines[i];

    if (line.startsWith('diff --git ')) {
      flushFile();
      continue;
    }

    if (line.startsWith('--- ')) {
      oldPath = normalizeDiffPath(line.slice(4).trim());
      continue;
    }

    if (line.startsWith('+++ ')) {
      const newPath = normalizeDiffPath(line.slice(4).trim());
      const resolved = newPath ?? oldPath;

      flushFile(); // save previous file

      if (resolved) {
        current = {
          file: resolved,
          hunks: [],
          createIfMissing: oldPath == null,
        };
        oldPath = newPath;
      }
      continue;
    }

    if (line.startsWith('@@')) {
      flushHunk();
      hunkAccum = [line];
      continue;
    }

    if (hunkAccum.length > 0) {
      hunkAccum.push(line);
    }
  }

  flushFile();
  return patches;
}

/** Strip `a/` or `b/` prefix; return `null` for `/dev/null`. */
function normalizeDiffPath(raw: string): string | null {
  const trimmed = raw.trim();
  if (!trimmed || trimmed === '/dev/null' || trimmed === 'dev/null') return null;
  return trimmed.replace(/^[ab]\//, '');
}

/**
 * Parse a hunk header `@@ -oldStart[,oldCount] +newStart[,newCount] @@`
 * followed by its body lines into a `Hunk` struct.
 */
function parseHunkLines(lines: string[]): Hunk | null {
  if (lines.length === 0) return null;

  const header = lines[0];
  const m = header.match(
    /^@@\s+-(\d+)(?:,(\d+))?\s+\+(\d+)(?:,(\d+))?\s+@@/,
  );
  if (!m) return null;

  const oldStart = parseInt(m[1], 10);
  const oldCount = m[2] ? parseInt(m[2], 10) : 1;
  const newStart = parseInt(m[3], 10);
  const newCount = m[4] ? parseInt(m[4], 10) : 1;

  const hunkLines: HunkLine[] = [];
  for (let i = 1; i < lines.length; i++) {
    const raw = lines[i];
    if (
      raw.startsWith('@@') ||
      raw.startsWith('diff ') ||
      raw.startsWith('--- ') ||
      raw.startsWith('+++ ')
    ) {
      break;
    }
    if (raw.startsWith('\\')) continue; // "No newline at end of file"

    const kind: HunkLineKind = raw.startsWith('-')
      ? 'remove'
      : raw.startsWith('+')
        ? 'add'
        : 'context';
    const content = raw.length > 0 ? raw.slice(1) : '';
    hunkLines.push({ kind, content });
  }

  return { oldStart, oldCount, newStart, newCount, lines: hunkLines };
}

// ── In-memory hunk application (mirrors Rust-side logic) ────────────────────

/**
 * Apply a list of hunks to `original` file content and return the result.
 * Tracks a cumulative line offset so multiple hunks targeting the same file
 * apply correctly even after prior hunks shift the line numbers.
 */
function applyHunks(original: string, hunks: Hunk[]): string {
  if (hunks.length === 0) return original;

  const lines = original === '' ? [] : original.split('\n');
  let cumulativeOffset = 0;

  for (const hunk of hunks) {
    const oldLines: string[] = [];
    const newLines: string[] = [];

    for (const hl of hunk.lines) {
      if (hl.kind === 'context' || hl.kind === 'remove') {
        oldLines.push(hl.content);
      }
      if (hl.kind === 'context' || hl.kind === 'add') {
        newLines.push(hl.content);
      }
    }

    const baseIdx = hunk.oldStart > 0 ? hunk.oldStart - 1 : 0;
    const adjustedIdx = Math.max(0, baseIdx + cumulativeOffset);

    const matchPos = findMatchPosition(lines, oldLines, adjustedIdx);
    if (matchPos === -1) {
      throw new Error(
        `Failed to match hunk at original line ${hunk.oldStart} ` +
          `(adjusted to line ${adjustedIdx + 1}). ` +
          `Expected context: ${JSON.stringify(oldLines.slice(0, 3))}${oldLines.length > 3 ? ' …' : ''}`,
      );
    }

    lines.splice(matchPos, oldLines.length, ...newLines);
    cumulativeOffset += newLines.length - oldLines.length;
  }

  return lines.join('\n');
}

/**
 * Find where `oldLines` occur in `lines`, preferring `preferredIdx`.
 * Returns a 0-based position or -1 if no match is found.
 *
 * First tries the preferred position exactly, then searches within a
 * small radius (fuzz matching, matching the Rust-side behaviour).
 */
function findMatchPosition(
  lines: string[],
  oldLines: string[],
  preferredIdx: number,
): number {
  if (oldLines.length === 0) {
    return Math.min(preferredIdx, lines.length);
  }

  if (matchesAt(lines, oldLines, preferredIdx)) {
    return preferredIdx;
  }

  // Fuzzy search (radius 3 matches the default Rust-side fuzz cap).
  const radius = 3;
  const lo = Math.max(0, preferredIdx - radius);
  const hi = Math.min(lines.length - oldLines.length, preferredIdx + radius);
  for (let i = lo; i <= hi; i++) {
    if (i !== preferredIdx && matchesAt(lines, oldLines, i)) {
      return i;
    }
  }

  return -1;
}

function matchesAt(lines: string[], oldLines: string[], pos: number): boolean {
  if (pos + oldLines.length > lines.length) return false;
  for (let i = 0; i < oldLines.length; i++) {
    if (lines[pos + i].trimEnd() !== oldLines[i].trimEnd()) return false;
  }
  return true;
}

// ── VS Code helpers ─────────────────────────────────────────────────────────

/**
 * Compute a `vscode.Range` covering the entire document content.
 */
function fullDocumentContentRange(content: string): vscode.Range {
  if (content.length === 0) {
    return new vscode.Range(0, 0, 0, 0);
  }
  const lines = content.split('\n');
  const lastIdx = lines.length - 1;
  const lastLen = lines[lastIdx].length;
  return new vscode.Range(0, 0, lastIdx, lastLen);
}
