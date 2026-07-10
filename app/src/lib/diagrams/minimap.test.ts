import { describe, expect, it } from 'vitest';
import { MAX_SCALE, MIN_SCALE, initialViewport, type Viewport } from './viewport';
import {
  clampZoomFactor,
  clampedViewportRect,
  clickToPan,
  fitWorldToMiniMap,
  hasWorldBox,
  miniPointToWorld,
  panToCenterWorld,
  visibleWorldRect,
  worldRectToMini,
} from './minimap';

/// A concrete world box for the geometry tests: an 800×600 SVG fitted into
/// the stage, viewed at the identity transform unless a test says otherwise.
function vp(over: Partial<Viewport> = {}): Viewport {
  return { scale: 1, tx: 0, ty: 0, baseW: 800, baseH: 600, ...over };
}

describe('hasWorldBox', () => {
  it('is false before the diagram is measured', () => {
    expect(hasWorldBox(initialViewport())).toBe(false);
  });
  it('is false with only one dimension set', () => {
    expect(hasWorldBox(vp({ baseH: 0 }))).toBe(false);
  });
  it('is true once both base dimensions are positive', () => {
    expect(hasWorldBox(vp())).toBe(true);
  });
});

describe('fitWorldToMiniMap', () => {
  it('scales the world box to fit inside the card, letter-boxing to keep aspect', () => {
    // 800×600 into a 200×200 card: limiting dimension is width → scale 0.25,
    // content 200×150, centred vertically (25px top/bottom bars).
    const fit = fitWorldToMiniMap(vp(), 200, 200);
    expect(fit.scale).toBeCloseTo(0.25, 6);
    expect(fit.contentW).toBeCloseTo(200, 6);
    expect(fit.contentH).toBeCloseTo(150, 6);
    expect(fit.offsetX).toBeCloseTo(0, 6);
    expect(fit.offsetY).toBeCloseTo(25, 6);
  });

  it('centres horizontally when height is the limiting dimension', () => {
    // 600×800 into 200×200: scale 0.25, content 150×200, 25px side bars.
    const fit = fitWorldToMiniMap(vp({ baseW: 600, baseH: 800 }), 200, 200);
    expect(fit.scale).toBeCloseTo(0.25, 6);
    expect(fit.offsetX).toBeCloseTo(25, 6);
    expect(fit.offsetY).toBeCloseTo(0, 6);
  });

  it('returns a degenerate fit for an unmeasured / zero-size world box', () => {
    expect(fitWorldToMiniMap(initialViewport(), 200, 200).scale).toBe(0);
    expect(fitWorldToMiniMap(vp(), 0, 200).scale).toBe(0);
  });
});

describe('visibleWorldRect', () => {
  it('covers the whole world box at the identity transform when stage == base', () => {
    const rect = visibleWorldRect(vp(), 800, 600);
    // `+0` normalises the signed zero `-tx/scale` produces at tx = 0.
    expect(rect.x + 0).toBe(0);
    expect(rect.y + 0).toBe(0);
    expect(rect.w).toBe(800);
    expect(rect.h).toBe(600);
  });

  it('shrinks and shifts as the stage zooms + pans in', () => {
    // scale 2, panned so world (100,50) sits at screen origin.
    const rect = visibleWorldRect(vp({ scale: 2, tx: -200, ty: -100 }), 800, 600);
    // world origin of stage = (-tx/scale, -ty/scale) = (100, 50)
    expect(rect.x).toBeCloseTo(100, 6);
    expect(rect.y).toBeCloseTo(50, 6);
    // world size = stage / scale = (400, 300)
    expect(rect.w).toBeCloseTo(400, 6);
    expect(rect.h).toBeCloseTo(300, 6);
  });
});

describe('worldRectToMini / miniPointToWorld round-trip', () => {
  it('projects a world rect through the fit', () => {
    const fit = fitWorldToMiniMap(vp(), 200, 200); // scale 0.25, offY 25
    const mini = worldRectToMini({ x: 100, y: 50, w: 400, h: 300 }, fit);
    expect(mini.x).toBeCloseTo(25, 6); // 0 + 100*0.25
    expect(mini.y).toBeCloseTo(37.5, 6); // 25 + 50*0.25
    expect(mini.w).toBeCloseTo(100, 6);
    expect(mini.h).toBeCloseTo(75, 6);
  });

  it('miniPointToWorld inverts the fit projection', () => {
    const fit = fitWorldToMiniMap(vp(), 200, 200);
    // Mini point (25, 37.5) should map back to world (100, 50).
    const world = miniPointToWorld(25, 37.5, fit);
    expect(world.x).toBeCloseTo(100, 6);
    expect(world.y).toBeCloseTo(50, 6);
  });

  it('miniPointToWorld is safe for a degenerate fit', () => {
    expect(miniPointToWorld(10, 10, fitWorldToMiniMap(initialViewport(), 200, 200))).toEqual({
      x: 0,
      y: 0,
    });
  });
});

describe('clampedViewportRect', () => {
  it('leaves a comfortably-sized rect untouched', () => {
    const fit = fitWorldToMiniMap(vp(), 200, 200);
    const world = { x: 100, y: 50, w: 400, h: 300 };
    const r = clampedViewportRect(world, fit, 8);
    // 100×75 mini rect is well above the 8px floor.
    expect(r).toEqual(worldRectToMini(world, fit));
  });

  it('grows a sub-minimum rect to the floor, centred on its true middle', () => {
    const fit = fitWorldToMiniMap(vp(), 200, 200); // scale 0.25
    // A tiny 4×4 world rect → 1×1 mini rect, below an 8px floor.
    const world = { x: 400, y: 300, w: 4, h: 4 };
    const raw = worldRectToMini(world, fit); // 1×1 at (offX+100, offY+75)
    const r = clampedViewportRect(world, fit, 8);
    expect(r.w).toBe(8);
    expect(r.h).toBe(8);
    // Centre preserved: expanded rect straddles the raw rect's centre.
    expect(r.x + r.w / 2).toBeCloseTo(raw.x + raw.w / 2, 6);
    expect(r.y + r.h / 2).toBeCloseTo(raw.y + raw.h / 2, 6);
  });
});

describe('panToCenterWorld', () => {
  it('centres the stage on a world point at the current scale', () => {
    // Stage 800×600, scale 1: to centre world (100,50) → tx=400-100, ty=300-50.
    const { tx, ty } = panToCenterWorld(vp(), 100, 50, 800, 600);
    expect(tx).toBeCloseTo(300, 6);
    expect(ty).toBeCloseTo(250, 6);
    // Verify: applying this pan, world (100,50) lands at stage centre.
    const screenX = 100 * 1 + tx;
    const screenY = 50 * 1 + ty;
    expect(screenX).toBeCloseTo(400, 6);
    expect(screenY).toBeCloseTo(300, 6);
  });

  it('accounts for the current scale', () => {
    const { tx, ty } = panToCenterWorld(vp({ scale: 2 }), 100, 50, 800, 600);
    expect(tx).toBeCloseTo(400 - 200, 6); // 400 - 100*2
    expect(ty).toBeCloseTo(300 - 100, 6); // 300 - 50*2
  });
});

describe('clickToPan', () => {
  it('composes mini→world→pan so a click centres the stage there', () => {
    const v = vp({ scale: 2, tx: -200, ty: -100 });
    const fit = fitWorldToMiniMap(v, 200, 200); // scale 0.25, offY 25
    // Click at the mini-map centre (100,100): world = ((100-0)/.25, (100-25)/.25)
    //   = (400, 300) — the middle of the 800×600 world box.
    const { tx, ty } = clickToPan(v, 100, 100, fit, 800, 600);
    // Stage centre should now sit on world (400,300): screen = world*2 + t = 400/300.
    expect(400 * 2 + tx).toBeCloseTo(400, 6);
    expect(300 * 2 + ty).toBeCloseTo(300, 6);
  });
});

describe('clampZoomFactor', () => {
  it('passes the factor through inside the bounds', () => {
    expect(clampZoomFactor(1, 2)).toBeCloseTo(2, 6);
    expect(clampZoomFactor(2, 0.5)).toBeCloseTo(0.5, 6);
  });

  it('caps the effective factor at MAX_SCALE', () => {
    // From scale 6, a ×4 would overshoot MAX_SCALE(8) → effective ×(8/6).
    const f = clampZoomFactor(6, 4);
    expect(6 * f).toBeCloseTo(MAX_SCALE, 6);
  });

  it('floors the effective factor at MIN_SCALE', () => {
    // From scale 0.4, a ×0.1 would undershoot MIN_SCALE(0.2) → effective ×0.5.
    const f = clampZoomFactor(0.4, 0.1);
    expect(0.4 * f).toBeCloseTo(MIN_SCALE, 6);
  });
});
