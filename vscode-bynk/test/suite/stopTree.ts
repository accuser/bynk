// #2069: stop a `wrangler dev` child and the workerd it started, and nothing
// else on the machine.
//
// The child is spawned `detached` (its own process group, off Windows), and
// workerd, a grandchild, inherits that group, so signalling the group
// (`process.kill(-pid)`) reaches exactly that tree. This replaces a
// `pkill -f workerd`, which also killed every other test's workerd and any
// `bynk dev` running alongside.

import { ChildProcess } from "child_process";

function signalGroup(pid: number, signal: NodeJS.Signals | 0): boolean {
  try {
    process.kill(-pid, signal);
    return true;
  } catch {
    return false; // ESRCH: no member of the group is left.
  }
}

export async function stopTree(child: ChildProcess | undefined): Promise<void> {
  if (!child || child.pid === undefined) return;
  if (process.platform === "win32") {
    child.kill();
    return;
  }
  const pid = child.pid;
  if (!signalGroup(pid, "SIGTERM")) return;
  const deadline = Date.now() + 5_000;
  while (Date.now() < deadline) {
    await new Promise((r) => setTimeout(r, 100));
    if (!signalGroup(pid, 0)) return;
  }
  signalGroup(pid, "SIGKILL");
}
