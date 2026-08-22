# Historical / Research — organization SSO

> **Unshipped service concept.** LeanCTX does not currently offer public
> organization accounts, SSO, SCIM, RBAC, billing plans, or a hosted enterprise
> control plane.

This file preserves the boundary around an earlier design. It is not a setup
guide, a security statement, an entitlement description, or a claim that an
OIDC endpoint exists.

## Current boundary

LeanCTX remains local-first and useful without an account. Local Runtime safety
and configuration do not depend on a hosted identity system.

Any future organization identity feature would require an explicit product
decision, accountable service/security ownership, a published support and
recovery model, and the evidence gate defined by the internal Product
Architecture. Until that happens, do not direct users to configure an IdP,
domain verification, pricing plan, or hosted LeanCTX login.
