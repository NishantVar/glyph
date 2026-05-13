---
name: main
description: 'Branch on risk and complexity.'
---

## Parameters

- **risk** (String). Default: "low".

## Steps

1. If risk == "high" and the requested change spans multiple files:
   a. Escalate to the architect.
   Otherwise:
   a. Proceed with the standard review.

