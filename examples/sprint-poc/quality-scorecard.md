# Human review scorecard

Use after the automated `expected-findings-v1` gate. A cheaper treatment is
not a win unless this page is accepted for **both** arms.

Workload: `fixture/checkout.py`<br>
Reviewer: _________________  Date: _________________

| Criterion | Stock (1–5) | Treatment (1–5) | Notes |
|---|---|---|---|
| Relevance (findings match real defects) | | | |
| Correctness (locations and cause are right) | | | |
| Actionable (a developer could fix from the text) | | | |

Pass: each arm ≥ 4 on every row, or write why a 3 is still accepted.

Automated gate: stock PASS/FAIL ____  treatment PASS/FAIL ____

Human accept stock? yes / no<br>
Human accept treatment? yes / no<br>

Savings claim allowed? yes / no
