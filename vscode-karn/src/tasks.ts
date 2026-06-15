// A build task that type-checks the whole project with `karnc check . --format
// short`, wired to the `$karnc` problem-matcher so errors land in the Problems
// panel. The LSP already reports diagnostics for *open* files; this catches the
// rest (unopened files, project-level errors) on demand.

import * as vscode from "vscode";

const TASK_TYPE = "karnc";

/** The compiler command — `karn.compilerPath` setting, else `karnc` on PATH. */
function compilerPath(): string {
  return (
    vscode.workspace
      .getConfiguration("karn")
      .get<string>("compilerPath", "")
      .trim() || "karnc"
  );
}

/** The `karnc: check` build task: `<karnc> check . --format short`, run at the
 *  workspace root, errors routed through `$karnc`. */
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
    "karnc",
    exec,
    ["$karnc"],
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
