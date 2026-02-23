---
name: test
description: Run the full rmus test suite with correct flags
---

Run the project test suite using the CI-equivalent command:

```bash
cargo test --locked --all-features --all-targets
```

After running:
- Report a pass/fail summary
- If tests fail, analyze the output and suggest fixes
- If compilation fails, identify the errors and propose corrections
