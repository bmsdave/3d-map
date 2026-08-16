# Maps SDK v2 browser lab

The lab is the executable demo and browser test surface for the v0.1.0-alpha
SDK. It renders only committed deterministic synthetic fixtures; it is not a
real-world map.

## Run

```sh
npm ci
npm run dev
```

Visit `http://localhost:5178`. Build and test with `npm run build` and
`npm run test:e2e`. The first command compiles the Rust/Wasm SDK and regenerates
the deterministic local fixture output, so no proprietary data or pipeline
cache is required.

Each card has a direct route at `/#/card/<id>`:

- `zoom-bands`, six `toggle-*` cards, and `globe-transition` exercise scale and
  globe transitions.
- `roads-micro` exercises synthetic pathologies: joins, casing, bridges,
  tunnels, and one upright midpoint road name — not real roads or curved line
  labels.
- `type-specimen`, `labels-collision`, `poi-density`, and
  `viewport-stability` exercise deterministic point-label placement.
- `input-flat`, `terrain-shade`, and `globe-relief` cover browser input and
  synthetic relief.

`labels-line`, `labels-poi`, and `roads-real` deliberately have no routes:
they are future work, not hidden features of this alpha. See the
[release boundary](../../libraries/maps-v2/docs/release-boundary.md).

## Browser-test contract

Cards expose state through `data-*` attributes and `data-testid` readouts.
Label tests assert placement invariants rather than the unstable claim that a
particular label must be visible. Golden screenshots are intentional rendering
contracts; review them visually. The `rendering-budget` test invokes an
on-demand p95 measurement across rendering cards and requires 10 ms or less.
