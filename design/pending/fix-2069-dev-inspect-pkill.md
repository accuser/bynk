---
level: patch
changelog: The `dev_inspect` workerd test and the VS Code extension's two workerd debug tests stop only the `wrangler dev` process group they started, instead of running `pkill -f workerd`. That command killed every workerd on the machine, while the `wrangler` it had started (and the workerd that one respawned) leaked. `dev_inspect` also asks the OS for free ports instead of deriving them from its pid (#2069).
---
