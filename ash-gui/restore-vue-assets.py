#!/usr/bin/env python
"""Plan 065: restore handwritten vue assets that `auto gen` does not emit.

`auto gen` regenerates gen/front/vue/src from .at sources but never produces
`src/lib/api.ts`, `src/lib/utils.ts`, or `src/components/ui/**` (shadcn-vue
primitives). A fresh gen therefore leaves the tree without them and
`vue-tsc && vite build` fails with TS2305/TS2307 until they are manually
re-patched (063/064 each hit this: ai_pending/ai_next/ai_steps/boot_script
endpoints had to be re-added by hand).

Flow: the canonical copies live TRACKED in ash-gui/vue-handwritten/.

    python restore-vue-assets.py            # push: canonical -> gen (after `auto gen`)
    python restore-vue-assets.py --pull     # pull: gen -> canonical (after hand-editing in gen/)
    python restore-vue-assets.py --check    # verify no drift, exit 1 on diff (CI guard)

Hand-edit workflow stays in gen/ (that is where vite/dev runs); --pull
persists the change so the next fresh gen restores it.
"""

import argparse
import filecmp
import shutil
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
CANONICAL = HERE / "vue-handwritten"
PROJECT = HERE / "ash-gui-auto"
DST_ROOT = PROJECT / "gen" / "front" / "vue" / "src"

# (canonical-relative, gen-relative) — both under src/.
ASSETS = {
    "lib/api.ts": "lib/api.ts",
    "lib/utils.ts": "lib/utils.ts",
    "components/ui": "components/ui",  # whole tree
}


def _pairs():
    for canon_rel, dst_rel in ASSETS.items():
        yield CANONICAL / canon_rel, DST_ROOT / dst_rel


def _copy(src: Path, dst: Path, direction: str):
    if src.is_dir():
        if dst.exists():
            shutil.rmtree(dst)
        shutil.copytree(src, dst)
    else:
        dst.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(src, dst)
    print(f"[{direction}] {src.name} -> {dst}" if direction == "push"
          else f"[{direction}] {dst} -> {src}")


def _same(a: Path, b: Path) -> bool:
    if a.is_dir():
        if not b.is_dir():
            return False
        files_a = sorted(p.relative_to(a) for p in a.rglob("*") if p.is_file())
        files_b = sorted(p.relative_to(b) for p in b.rglob("*") if p.is_file())
        if files_a != files_b:
            return False
        return all(filecmp.cmp(a / f, b / f, shallow=False) for f in files_a)
    return b.is_file() and filecmp.cmp(a, b, shallow=False)


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--pull", action="store_true",
                    help="gen -> canonical (persist hand edits made in gen/)")
    ap.add_argument("--check", action="store_true",
                    help="fail (exit 1) if canonical and gen differ")
    args = ap.parse_args()

    if not CANONICAL.exists():
        print(f"canonical store missing: {CANONICAL}", file=sys.stderr)
        return 1
    if not DST_ROOT.exists():
        print(f"gen tree missing (run `auto gen` first): {DST_ROOT}", file=sys.stderr)
        return 1

    if args.check:
        drift = [str(a.relative_to(CANONICAL)) for a, b in _pairs() if not _same(a, b)]
        if drift:
            print("DRIFT between ash-gui/vue-handwritten and gen/front/vue/src:")
            for d in drift:
                print("  -", d)
            return 1
        print("OK: handwritten assets in sync")
        return 0

    for a, b in _pairs():
        if args.pull:
            src, dst = b, a
            if not b.exists():
                print(f"[pull] SKIP missing {b}", file=sys.stderr)
                continue
            _copy(src, dst, "pull")
        else:
            _copy(a, b, "push")
    print("done:", "pull" if args.pull else "push")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
