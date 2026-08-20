import * as assert from "assert";
import * as vscode from "vscode";

suite("Extension Test Suite", () => {
  test("Extension should be present", () => {
    const ext = vscode.extensions.getExtension("LeanCTX.lean-ctx");
    assert.ok(ext, "Extension not found");
  });

  test("Extension should activate", async () => {
    const ext = vscode.extensions.getExtension("LeanCTX.lean-ctx");
    if (ext && !ext.isActive) {
      await ext.activate();
    }
    assert.ok(ext?.isActive, "Extension did not activate");
  });

  test("Commands should be registered", async () => {
    const commands = await vscode.commands.getCommands(true);
    const leanctxCommands = commands.filter((c) => c.startsWith("leanctx."));
    assert.ok(leanctxCommands.length > 0, "No lean-ctx commands registered");
  });
});
