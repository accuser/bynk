---
level: patch
changelog: A `messages` `select` placeholder now dispatches its arm by own-property check, so a `MessageArg.Text` value naming an `Object.prototype` member (`"constructor"`, `"toString"`, `"__proto__"`) falls back to the mandatory `other` arm instead of resolving off the prototype chain
---
