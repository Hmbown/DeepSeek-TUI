import * as vscode from 'vscode';

interface Hunk {
  oldStart: number;
  oldLines: string[];
  newLines: string[];
}

interface FilePatch {
  file: string;
  hunks: Hunk[];
}

function parsePatch(patch: string): FilePatch[] {
  const files: FilePatch[] = [];
  let current: FilePatch | null = null;
  let hunk: Hunk | null = null;

  for (const line of patch.split('\n')) {
    if (line.startsWith('+++ ')) {
      const file = line.slice(4).replace(/^b\//, '');
      current = { file, hunks: [] };
      files.push(current);
    } else if (line.startsWith('@@ ') && current) {
      const m = line.match(/@@ -(\d+)/);
      hunk = { oldStart: m ? parseInt(m[1]) - 1 : 0, oldLines: [], newLines: [] };
      current.hunks.push(hunk);
    } else if (hunk) {
      if (line.startsWith('-')) hunk.oldLines.push(line.slice(1));
      else if (line.startsWith('+')) hunk.newLines.push(line.slice(1));
      else if (line.startsWith(' ')) { hunk.oldLines.push(line.slice(1)); hunk.newLines.push(line.slice(1)); }
    }
  }
  return files;
}

export async function applyPatchAsWorkspaceEdit(patch: string, directWrite = false): Promise<void> {
  const patches = parsePatch(patch);
  const edit = new vscode.WorkspaceEdit();

  for (const fp of patches) {
    const uri = vscode.Uri.file(fp.file);
    const doc = await vscode.workspace.openTextDocument(uri);
    for (const hunk of fp.hunks) {
      const range = new vscode.Range(hunk.oldStart, 0, hunk.oldStart + hunk.oldLines.length, 0);
      edit.replace(uri, range, hunk.newLines.join('\n') + '\n');
    }
  }

  if (directWrite) {
    await vscode.workspace.applyEdit(edit);
  } else {
    await vscode.workspace.applyEdit(edit);
    // VS Code shows diff preview automatically via WorkspaceEdit
  }
}
