import { describe, expect, it } from 'vitest';
import {
  clampStep,
  DEFAULT_SCALE,
  initPresenter,
  isFirstStep,
  isLastStep,
  keyToAction,
  nextScale,
  prevScale,
  reduce,
  SCALE_PRESETS,
  stepCounter,
  type PresenterState,
} from './presenter';

function state(overrides: Partial<PresenterState> = {}): PresenterState {
  return { ...initPresenter(12, 2), ...overrides };
}

describe('clampStep', () => {
  it('keeps an in-range step', () => {
    expect(clampStep(3, 12)).toBe(3);
  });
  it('clamps below zero to zero', () => {
    expect(clampStep(-5, 12)).toBe(0);
  });
  it('clamps past the end to the last index', () => {
    expect(clampStep(99, 12)).toBe(11);
  });
  it('returns 0 for an empty tour', () => {
    expect(clampStep(4, 0)).toBe(0);
  });
});

describe('stepCounter', () => {
  it('formats a 1-based counter', () => {
    expect(stepCounter({ step: 2, total: 12 })).toBe('3 / 12');
  });
  it('reads 0 / 0 for an empty tour', () => {
    expect(stepCounter({ step: 0, total: 0 })).toBe('0 / 0');
  });
  it('shows the last step correctly', () => {
    expect(stepCounter({ step: 11, total: 12 })).toBe('12 / 12');
  });
});

describe('isFirstStep / isLastStep', () => {
  it('detects the first step', () => {
    expect(isFirstStep({ step: 0 })).toBe(true);
    expect(isFirstStep({ step: 1 })).toBe(false);
  });
  it('detects the last step', () => {
    expect(isLastStep({ step: 11, total: 12 })).toBe(true);
    expect(isLastStep({ step: 5, total: 12 })).toBe(false);
  });
  it('an empty tour has no last step', () => {
    expect(isLastStep({ step: 0, total: 0 })).toBe(false);
  });
});

describe('nextScale', () => {
  it('cycles through the presets in order', () => {
    expect(nextScale(1.0)).toBe(1.25);
    expect(nextScale(1.25)).toBe(1.5);
  });
  it('wraps from the largest back to the smallest', () => {
    expect(nextScale(1.5)).toBe(1.0);
  });
  it('falls back to the first preset for an unknown scale', () => {
    expect(nextScale(0.7)).toBe(SCALE_PRESETS[0]);
  });
});

describe('prevScale', () => {
  it('cycles backwards through the presets', () => {
    expect(prevScale(1.5)).toBe(1.25);
    expect(prevScale(1.25)).toBe(1.0);
  });
  it('wraps from the smallest back to the largest', () => {
    expect(prevScale(1.0)).toBe(1.5);
  });
});

describe('keyToAction', () => {
  it('maps arrows to navigation', () => {
    expect(keyToAction('ArrowRight')).toEqual({ type: 'next' });
    expect(keyToAction('ArrowLeft')).toEqual({ type: 'prev' });
  });
  it('maps n to the narrator toggle (either case)', () => {
    expect(keyToAction('n')).toEqual({ type: 'toggleNarrator' });
    expect(keyToAction('N')).toEqual({ type: 'toggleNarrator' });
  });
  it('maps r and p to the overlay toggles', () => {
    expect(keyToAction('r')).toEqual({ type: 'toggleOverlay', overlay: 'risk' });
    expect(keyToAction('p')).toEqual({ type: 'toggleOverlay', overlay: 'pattern' });
  });
  it('maps Escape to exit', () => {
    expect(keyToAction('Escape')).toEqual({ type: 'exit' });
  });
  it('maps +/-/0 to scale controls', () => {
    expect(keyToAction('+')).toEqual({ type: 'cycleScale' });
    expect(keyToAction('-')).toEqual({ type: 'cycleScaleDown' });
    expect(keyToAction('0')).toEqual({ type: 'setScale', scale: DEFAULT_SCALE });
  });
  it('returns null for unbound keys', () => {
    expect(keyToAction('x')).toBeNull();
    expect(keyToAction('Enter')).toBeNull();
  });
});

describe('reduce', () => {
  it('enter activates without touching the pointer', () => {
    const s = reduce(state({ active: false }), { type: 'enter' });
    expect(s.active).toBe(true);
    expect(s.step).toBe(2);
  });

  it('exit deactivates and clears overlays but keeps scale', () => {
    const s = reduce(
      state({ active: true, scale: 1.5, overlays: new Set(['risk']) }),
      { type: 'exit' },
    );
    expect(s.active).toBe(false);
    expect(s.overlays.size).toBe(0);
    expect(s.scale).toBe(1.5);
  });

  it('next advances but stops at the last step', () => {
    let s = state({ step: 10 });
    s = reduce(s, { type: 'next' });
    expect(s.step).toBe(11);
    s = reduce(s, { type: 'next' });
    expect(s.step).toBe(11); // clamped
  });

  it('prev goes back but stops at the first step', () => {
    let s = state({ step: 1 });
    s = reduce(s, { type: 'prev' });
    expect(s.step).toBe(0);
    s = reduce(s, { type: 'prev' });
    expect(s.step).toBe(0); // clamped
  });

  it('toggleNarrator flips the flag', () => {
    const on = reduce(state({ narrator: false }), { type: 'toggleNarrator' });
    expect(on.narrator).toBe(true);
    const off = reduce(on, { type: 'toggleNarrator' });
    expect(off.narrator).toBe(false);
  });

  it('toggleOverlay adds then removes an overlay', () => {
    const withRisk = reduce(state(), { type: 'toggleOverlay', overlay: 'risk' });
    expect(withRisk.overlays.has('risk')).toBe(true);
    const without = reduce(withRisk, { type: 'toggleOverlay', overlay: 'risk' });
    expect(without.overlays.has('risk')).toBe(false);
  });

  it('overlays are independent', () => {
    let s = reduce(state(), { type: 'toggleOverlay', overlay: 'risk' });
    s = reduce(s, { type: 'toggleOverlay', overlay: 'pattern' });
    expect(s.overlays.has('risk')).toBe(true);
    expect(s.overlays.has('pattern')).toBe(true);
  });

  it('cycleScale walks the presets', () => {
    let s = state({ scale: 1.0 });
    s = reduce(s, { type: 'cycleScale' });
    expect(s.scale).toBe(1.25);
  });

  it('cycleScaleDown walks the presets backwards', () => {
    let s = state({ scale: 1.25 });
    s = reduce(s, { type: 'cycleScaleDown' });
    expect(s.scale).toBe(1.0);
  });

  it('setTotal re-clamps the current step', () => {
    const s = reduce(state({ step: 10, total: 12 }), { type: 'setTotal', total: 5 });
    expect(s.total).toBe(5);
    expect(s.step).toBe(4);
  });

  it('setStep clamps to the tour bounds', () => {
    const s = reduce(state({ total: 12 }), { type: 'setStep', step: 100 });
    expect(s.step).toBe(11);
  });

  it('does not mutate the input state', () => {
    const before = state({ overlays: new Set(['risk']) });
    const snapshot = { ...before, overlays: new Set(before.overlays) };
    reduce(before, { type: 'toggleOverlay', overlay: 'pattern' });
    expect(before.overlays).toEqual(snapshot.overlays);
    expect(before.step).toBe(snapshot.step);
  });
});
