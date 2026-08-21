---
level: patch
changelog: "P7.7: named the bynk-ts printer's readability guarantee (R7.5) explicitly -- a `# Readability policy` doc block in printer.rs states the printer's one real formatting decision today (exactly one generated line per statement) and its boundary (a statement's own interior formatting isn't the printer's concern until Arc C gives it real nodes). No behaviour change (#1311)."
---
