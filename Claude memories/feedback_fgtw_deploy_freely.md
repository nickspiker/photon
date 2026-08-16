---
name: feedback_fgtw_deploy_freely
description: User authorizes deploying fgtw.org (wrangler) freely — no per-deploy confirmation needed
metadata: 
  node_type: memory
  type: feedback
  originSessionId: 0b164fd9-062c-407e-b4bb-8f6be8d1982d
---

Deploy fgtw.org / the fgtw-bootstrap Cloudflare Worker (wrangler) and the toka.wasm browser build freely, without stopping to confirm each deploy ("You can deploy all day long. doesn't hurt my feelers any.").

**Why:** it's the user's own site/infra and they treat deploys as cheap/reversible; gating on confirmation just slows iteration.

**How to apply:** when a task's natural conclusion is a live fgtw.org deploy, just do it (build → deploy → verify) and report the result. Still report what was deployed. Does not extend to other outward-facing actions or other projects. Relates to [[project_peers_are_fgtw]].
