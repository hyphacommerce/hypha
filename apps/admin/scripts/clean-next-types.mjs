// Removes the route types Next.js and vinext generate under .next/ before `tsc` runs.
// Both write `.next/types` in their own shapes, and `next build` re-adds them to
// tsconfig's `include`, so after running one and then the other, `tsc` fails on the
// mismatch. The builds check their own route types; the typecheck doesn't need them.
import { rmSync } from "node:fs";

for (const dir of [".next/types", ".next/dev/types"]) {
  rmSync(dir, { recursive: true, force: true });
}
