import { describe, expect, it } from 'vitest';
import { get } from 'svelte/store';
import {
  MAX_SCALE,
  MIN_SCALE,
  createViewportStore,
  initialViewport,
  panBy,
  panTo,
  reset,
  setBaseSize,
  zoomAround,
} from './viewport';

describe('viewport reducers', () => {
  it('starts at the identity transform with no base size', () => {
    expect(initialViewport()).toEqual({ scale: 1, tx: 0, ty: 0, baseW: 0, baseH: 0 });
  });

  it('reset() clears pan + zoom but keeps the base size', () => {
    const v = { scale: 3, tx: 40, ty: -20, baseW: 800, baseH: 600 };
    expect(reset(v)).toEqual({ scale: 1, tx: 0, ty: 0, baseW: 800, baseH: 600 });
  });

  it('panBy() accumulates screen-space deltas', () => {
    const v = panBy(initialViewport(), 10, -5);
    expect(v).toMatchObject({ tx: 10, ty: -5 });
    expect(panBy(v, 3, 5)).toMatchObject({ tx: 13, ty: 0 });
  });

  it('panTo() sets an absolute offset', () => {
    expect(panTo({ ...initialViewport(), tx: 99, ty: 99 }, 7, 8)).toMatchObject({
      tx: 7,
      ty: 8,
    });
  });

  it('setBaseSize() leaves the live transform untouched', () => {
    const v = { scale: 2, tx: 5, ty: 6, baseW: 0, baseH: 0 };
    expect(setBaseSize(v, 320, 240)).toEqual({
      scale: 2,
      tx: 5,
      ty: 6,
      baseW: 320,
      baseH: 240,
    });
  });

  describe('zoomAround()', () => {
    it('scales by the factor when unclamped', () => {
      const v = zoomAround(initialViewport(), 2, 100, 100);
      expect(v.scale).toBe(2);
    });

    it('keeps the anchor world-point fixed on zoom in', () => {
      // At scale 1, tx/ty 0: the world-point under (100,100) is (100,100).
      // After zooming in it must still map back to screen (100,100).
      const v = zoomAround(initialViewport(), 2, 100, 100);
      // world = (screen - tx) / scale
      const worldX = (100 - v.tx) / v.scale;
      const worldY = (100 - v.ty) / v.scale;
      expect(worldX).toBeCloseTo(100, 6);
      expect(worldY).toBeCloseTo(100, 6);
    });

    it('keeps the anchor fixed even from a panned + zoomed start', () => {
      const start = { scale: 1.5, tx: 30, ty: -12, baseW: 0, baseH: 0 };
      const anchorX = 220;
      const anchorY = 80;
      const worldBefore = {
        x: (anchorX - start.tx) / start.scale,
        y: (anchorY - start.ty) / start.scale,
      };
      const v = zoomAround(start, 1.3, anchorX, anchorY);
      const worldAfter = {
        x: (anchorX - v.tx) / v.scale,
        y: (anchorY - v.ty) / v.scale,
      };
      expect(worldAfter.x).toBeCloseTo(worldBefore.x, 6);
      expect(worldAfter.y).toBeCloseTo(worldBefore.y, 6);
    });

    it('clamps at MAX_SCALE and still pins the anchor', () => {
      const start = { scale: MAX_SCALE, tx: 10, ty: 10, baseW: 0, baseH: 0 };
      const v = zoomAround(start, 4, 50, 50);
      expect(v.scale).toBe(MAX_SCALE);
      // ratio is 1 → transform unchanged, anchor trivially fixed.
      expect(v.tx).toBe(10);
      expect(v.ty).toBe(10);
    });

    it('clamps at MIN_SCALE on aggressive zoom out', () => {
      const v = zoomAround({ ...initialViewport(), scale: MIN_SCALE }, 0.1, 0, 0);
      expect(v.scale).toBe(MIN_SCALE);
    });
  });
});

describe('createViewportStore', () => {
  it('is a Svelte store starting from the initial viewport', () => {
    const store = createViewportStore();
    expect(get(store)).toEqual(initialViewport());
  });

  it('applies pan / zoom / reset / base-size through helpers', () => {
    const store = createViewportStore();
    store.setBaseSize(640, 480);
    store.panBy(20, 10);
    store.zoomAround(2, 0, 0);
    let v = get(store);
    expect(v.baseW).toBe(640);
    expect(v.scale).toBe(2);
    // panBy(20,10) then zoomAround factor 2 about origin: tx = 0 - (0-20)*2 = 40
    expect(v.tx).toBe(40);
    expect(v.ty).toBe(20);

    store.reset();
    v = get(store);
    expect(v).toMatchObject({ scale: 1, tx: 0, ty: 0, baseW: 640, baseH: 480 });
  });

  it('panTo sets an absolute offset via the store', () => {
    const store = createViewportStore();
    store.panTo(11, 22);
    expect(get(store)).toMatchObject({ tx: 11, ty: 22 });
  });
});
