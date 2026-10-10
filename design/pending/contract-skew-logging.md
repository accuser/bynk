---
level: patch
changelog: "Contract skew is now logged on both sides (#1826). A callee that refuses a mismatched `X-Bynk-Contract` logs `ContractMismatch <context> call <service>` with the service and both hashes before its `409` (the caller-supplied hash bounded to 16 characters, since nothing about the request is trusted yet), and the caller's `callService` logs the mismatch before it throws `BoundaryError: ContractMismatch`. Before, neither side logged anything, and a skewed deployment looked like any other unexplained `500`"
---
