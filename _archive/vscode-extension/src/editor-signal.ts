import * as vscode from "vscode";
import { runLeanCtx } from "./leanctx";

/**
 * Editor focus signal (#500): report the active file to lean-ctx so the
 * context engine can rank it up in preload/triage. Paths only — never
 * content — and only files inside the current workspace.
 */

const DEBOUNCE_MS = 2_000;

let debounceTimer: ReturnType<typeof setTimeout> | undefined;
let lastSent: string | undefined;

function isWorkspaceFile(fsPath: string): boolean {
  const folders = vscode.workspace.workspaceFolders;
  if (!folders || folders.length === 0) {
    return false;
  }
  return folders.some((f) => fsPath.startsWith(f.uri.fsPath));
}

function sendSignal(fsPath: string): void {
  if (fsPath === lastSent) {
    return;
  }
  lastSent = fsPath;
  // Fire-and-forget: a lost signal is harmless (next tab change resends),
  // and the editor must never block on it.
  void runLeanCtx(["editor-signal", "--file", fsPath]).catch(() => {
    /* binary missing or old version — silently inert */
  });
}

function onEditorChange(editor: vscode.TextEditor | undefined): void {
  const enabled = vscode.workspace
    .getConfiguration("leanctx")
    .get<boolean>("editorSignal.enabled", true);
  if (!enabled) {
    return;
  }

  const doc = editor?.document;
  // Real files only: no output panes, git views, untitled buffers etc.
  if (!doc || doc.uri.scheme !== "file" || !isWorkspaceFile(doc.uri.fsPath)) {
    return;
  }

  const fsPath = doc.uri.fsPath;
  if (debounceTimer) {
    clearTimeout(debounceTimer);
  }
  debounceTimer = setTimeout(() => sendSignal(fsPath), DEBOUNCE_MS);
}

export function registerEditorSignal(context: vscode.ExtensionContext): void {
  context.subscriptions.push(
    vscode.window.onDidChangeActiveTextEditor(onEditorChange),
    {
      dispose: () => {
        if (debounceTimer) {
          clearTimeout(debounceTimer);
        }
      },
    }
  );
  // Report the file that is already open when the extension activates.
  onEditorChange(vscode.window.activeTextEditor);
}
