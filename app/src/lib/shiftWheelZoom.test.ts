/// Tests for the shared shift-wheel zoom helper.
///
/// The user-facing guarantee these tests pin down: "Shift+Wheel zooms in
/// every viewer that uses createShiftWheelZoom" — regardless of whether the
/// OS swapped deltaY → deltaX (macOS does, Linux/Windows don't).

import { describe, it, expect, beforeEach } from 'vitest';
import { clampZoom, readZoom, wheelDelta, writeZoom, createShiftWheelZoom } from './shiftWheelZoom';
import { get } from 'svelte/store';

describe('clampZoom', () => {
  it('clamps to [min, max]', () => {
    expect(clampZoom(0.1, 0.6, 2.0)).toBe(0.6);
    expect(clampZoom(5.0, 0.6, 2.0)).toBe(2.0);
    expect(clampZoom(1.0, 0.6, 2.0)).toBe(1.0);
  });

  it('rounds to two decimals', () => {
    expect(clampZoom(1.234567)).toBe(1.23);
    expect(clampZoom(1.235)).toBe(1.24);
  });
});

describe('wheelDelta', () => {
  function makeEvent(deltaY: number, deltaX: number, shiftKey = false): WheelEvent {
    return { deltaY, deltaX, shiftKey } as unknown as WheelEvent;
  }

  it('returns deltaY when it dominates (no axis swap)', () => {
    // Linux/Windows Shift+Wheel: deltaY survives, deltaX is 0
    expect(wheelDelta(makeEvent(120, 0, true))).toBe(120);
    expect(wheelDelta(makeEvent(-120, 0, true))).toBe(-120);
  });

  it('returns deltaX when the OS axis-swapped (macOS Shift+Wheel)', () => {
    // macOS Shift+Wheel: deltaY → deltaX
    expect(wheelDelta(makeEvent(0, 120, true))).toBe(120);
    expect(wheelDelta(makeEvent(0, -120, true))).toBe(-120);
  });

  it('returns 0 when both axes are 0', () => {
    expect(wheelDelta(makeEvent(0, 0))).toBe(0);
  });

  it('prefers the larger-magnitude axis when both are non-zero', () => {
    expect(wheelDelta(makeEvent(50, 200))).toBe(200);
    expect(wheelDelta(makeEvent(200, 50))).toBe(200);
  });
});

describe('readZoom / writeZoom', () => {
  beforeEach(() => {
    localStorage.clear();
  });

  it('returns 1.0 when the key is missing', () => {
    expect(readZoom('projectmind.test.missing')).toBe(1.0);
  });

  it('round-trips a clamped value', () => {
    writeZoom('projectmind.test.rt', 1.5);
    expect(readZoom('projectmind.test.rt')).toBe(1.5);
  });

  it('clamps stored values that fall outside [min, max] on read', () => {
    writeZoom('projectmind.test.clamp', 9.9);
    expect(readZoom('projectmind.test.clamp', 0.6, 2.0)).toBe(2.0);
  });

  it('returns 1.0 for unparseable storage', () => {
    localStorage.setItem('projectmind.test.bad', 'not-a-number');
    expect(readZoom('projectmind.test.bad')).toBe(1.0);
  });
});

describe('createShiftWheelZoom', () => {
  beforeEach(() => {
    localStorage.clear();
  });

  // The svelte action attaches a `wheel` listener to `window` and filters
  // by `node.contains(ev.target)`. happy-dom doesn't propagate node-level
  // dispatches up to the window the same way a real browser does, so the
  // tests synthesise the upward path: dispatch on `window` directly, with
  // the `target` overridden to the inside-node we want to model. That's
  // exactly what the production handler observes — `ev.target` is what the
  // listener inspects, and the window-level dispatch matches `addEventListener('wheel', ...)`.
  function fireWheel(target: Node, init: WheelEventInit) {
    // happy-dom's WheelEvent doesn't accept deltaX / deltaY through the
    // standard constructor reliably and doesn't propagate node dispatches
    // up to window — both gaps relative to a real browser. Fabricate the
    // event shape the production handler reads (deltaX/deltaY/shiftKey/target)
    // directly and dispatch on window, which is where the listener lives.
    const ev = new Event('wheel', { bubbles: true, cancelable: true }) as unknown as WheelEvent & {
      deltaX: number;
      deltaY: number;
      shiftKey: boolean;
    };
    ev.deltaX = init.deltaX ?? 0;
    ev.deltaY = init.deltaY ?? 0;
    ev.shiftKey = init.shiftKey ?? false;
    Object.defineProperty(ev, 'target', { value: target, configurable: true });
    window.dispatchEvent(ev);
  }

  it('zooms in when shift+wheel-up fires inside the bound node (Linux/Windows shape)', () => {
    const { zoom, action } = createShiftWheelZoom('projectmind.test.cswz1', { step: 0.1 });
    const host = document.createElement('div');
    document.body.appendChild(host);
    const handle = action(host);
    expect(get(zoom)).toBe(1.0);

    fireWheel(host, { deltaY: -120, deltaX: 0, shiftKey: true });

    expect(get(zoom)).toBeCloseTo(1.1, 5);
    handle.destroy();
    host.remove();
  });

  it('zooms in when shift+wheel arrives axis-swapped (macOS shape)', () => {
    const { zoom, action } = createShiftWheelZoom('projectmind.test.cswz2', { step: 0.1 });
    const host = document.createElement('div');
    document.body.appendChild(host);
    const handle = action(host);

    fireWheel(host, { deltaY: 0, deltaX: -120, shiftKey: true });

    expect(get(zoom)).toBeCloseTo(1.1, 5);
    handle.destroy();
    host.remove();
  });

  it('ignores wheel events without shift', () => {
    const { zoom, action } = createShiftWheelZoom('projectmind.test.cswz3', { step: 0.1 });
    const host = document.createElement('div');
    document.body.appendChild(host);
    const handle = action(host);

    fireWheel(host, { deltaY: -120, shiftKey: false });

    expect(get(zoom)).toBe(1.0);
    handle.destroy();
    host.remove();
  });

  it('ignores wheel events outside the bound node', () => {
    const { zoom, action } = createShiftWheelZoom('projectmind.test.cswz4', { step: 0.1 });
    const host = document.createElement('div');
    const outside = document.createElement('div');
    document.body.appendChild(host);
    document.body.appendChild(outside);
    const handle = action(host);

    fireWheel(outside, { deltaY: -120, shiftKey: true });

    expect(get(zoom)).toBe(1.0);
    handle.destroy();
    host.remove();
    outside.remove();
  });

  it('persists across instances via the same key', () => {
    const first = createShiftWheelZoom('projectmind.test.cswz5', { step: 0.1 });
    first.zoom.set(1.4);

    const second = createShiftWheelZoom('projectmind.test.cswz5', { step: 0.1 });
    expect(get(second.zoom)).toBe(1.4);
  });

  it('clamps imperative zoom writes', () => {
    const { zoom } = createShiftWheelZoom('projectmind.test.cswz6', {
      min: 0.5,
      max: 1.8,
      step: 0.1,
    });
    zoom.set(5.0);
    expect(get(zoom)).toBe(1.8);
    zoom.set(0.1);
    expect(get(zoom)).toBe(0.5);
  });
});
