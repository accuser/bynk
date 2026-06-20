// A build task that type-checks the whole project with `bynkc check . --format
// short`, wired to the `$bynkc` problem-matcher so errors land in the Problems
// panel. The LSP already reports diagnostics for *open* files; this catches the
// rest (unopened files, project-level errors) on demand.

import * as vscode from "vscode";

const TASK_TYPE = "bynkc";

/** The compiler command — `bynk.compilerPath` setting, else `bynkc` on PATH. */
export function compilerPath(): string {
  return (
    vscode.workspace
      .getConfiguration("bynk")
      .get<string>("compilerPath", "")
      .trim() || "bynkc"
  );
}

/** The `bynkc: check` build task: `<bynkc> check . --format short`, run at the
 *  workspace root, errors routed through `$bynkc`. */
function checkTask(definition: vscode.TaskDefinition = { type: TASK_TYPE }): vscode.Task {
  const exec = new vscode.ShellExecution(compilerPath(), [
    "check",
    ".",
    "--format",
    "short",
  ]);
  const task = new vscode.Task(
    definition,
    vscode.TaskScope.Workspace,
    "check",
    "bynkc",
    exec,
    ["$bynkc"],
  );
  task.group = vscode.TaskGroup.Build;
  return task;
}

export function registerTasks(context: vscode.ExtensionContext): void {
  context.subscriptions.push(
    vscode.tasks.registerTaskProvider(TASK_TYPE, {
      provideTasks: () => [checkTask()],
      resolveTask: (task) =>
        task.definition.type === TASK_TYPE ? checkTask(task.definition) : undefined,
    }),
  );
}
