# Skill: new-card

Create a new lab card with token efficiency.

Steps:
1. Read `applications/maps-v2-lab/src/cards/types.ts:1` (CardSpec) + one existing card (e.g. `roadsMicro.ts` or `buildings3d.ts` — pick closest group).
2. Copy pattern: `export const card: CardSpec = { id, title, purpose, group, mount }`.
3. Use `createPackageMap` from `src/sdk.ts:545` for real ground (Trafalgar), or `createMap` `sdk.ts:441` for synthetic fixture. Never inline tile fetch — use loader `sdk.ts:292`.
4. Register in `src/cards/index.ts`.
5. Verify: `cd applications/maps-v2-lab && npm run typecheck && npm run build`.
6. Do not read all 18 cards. Do not read 648-line sdk.ts fully — use AGENTS.md offsets.
