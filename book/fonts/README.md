# Vendored Source fonts

This directory contains the ten static OpenType faces used by the print
manuscript. Keeping this small, fixed subset with the source makes local and CI
builds use exactly the same metrics; `scripts/build-book.sh` ignores system
fonts and verifies every file against `SHA256SUMS` before compilation.

The files are unmodified selections from these Adobe releases:

| Family | Version | Upstream archive | Archive SHA-256 |
| --- | --- | --- | --- |
| Source Serif 4 | 4.005 | [source-serif-4.005_Desktop.zip](https://github.com/adobe-fonts/source-serif/releases/download/4.005R/source-serif-4.005_Desktop.zip) | `549fdb8f9a682bd06944298621404969f6de77c2e422ff3b8244a1dcd6a0c425` |
| Source Sans 3 | 3.052 | [OTF-source-sans-3.052R.zip](https://github.com/adobe-fonts/source-sans/releases/download/3.052R/OTF-source-sans-3.052R.zip) | `a4ebbdea20b08ccbd7bf3665a9462454eefdd01d9a6307129d3b3d4672981074` |
| Source Code Pro | 2.042 | [OTF-source-code-pro-2.042R-u_1.062R-i.zip](https://github.com/adobe-fonts/source-code-pro/releases/download/2.042R-u/1.062R-i/1.026R-vf/OTF-source-code-pro-2.042R-u_1.062R-i.zip) | `754a2e3ebb945ae905d720ac5896b3b34acc9546dd6551ef9536869788629dae` |

The fonts are copyright Adobe and distributed under the SIL Open Font License
1.1. See `LICENSE.md`. The reserved font name is “Source”.
