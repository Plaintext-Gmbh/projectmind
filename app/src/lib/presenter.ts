// Pure presenter-mode logic (Cockpit 2.6, #162).
//
// Presenter Mode turns the active walk-through into a full-screen slide deck:
// bigger fonts, sidebar hidden, a `3 / 12` step counter, and single-key
// navigation. All the *decisions* live here as pure functions so they can be
// unit-tested without a DOM — the Svelte component (`PresenterView.svelte`)
// only wires these to the screen and the TTS/overlay side effects.
//
// The keyboard contract (matches the #162 spec):
//   ← / →   previous / next step
//   n       toggle the TTS narrator
//   r       toggle the risk-badge overlay for the current class/file
//   p       toggle the pattern-compliance overlay for the current class
//   Esc     exit presenter mode
//   +/-/0   (bonus) bump the font scale up/down/reset — handy on a projector

/** Font-scale presets the `+`/`-` keys and the header cycle through. */
export const SCALE_PRESETS = [1.0, 1.25, 1.5] as const;

/** A scale value from {@link SCALE_PRESETS}. */
export type Scale = (typeof SCALE_PRESETS)[number];

/** The default presenter scale — normal size. */
export const DEFAULT_SCALE: Scale = 1.0;

/**
 * The two independent overlays a presenter can toggle on top of a step.
 * `risk` shows the risk-atlas badges for the current class/file; `pattern`
 * shows the pattern-compliance summary for the current class.
 */
export type Overlay = 'risk' | 'pattern';

/** Immutable presenter-mode state. Every reducer returns a fresh object. */
export interface PresenterState {
  /** Whether presenter mode is active (full-screen deck showing). */
  active: boolean;
  /** 0-based index of the current step. */
  step: number;
  /** Total number of steps in the tour (>= 0). */
  total: number;
  /** Current font scale. */
  scale: Scale;
  /** Whether the TTS narrator is enabled. */
  narrator: boolean;
  /** Which overlays are currently shown. */
  overlays: Set<Overlay>;
}

/** Build the initial presenter state for a tour of `total` steps. */
export function initPresenter(total: number, step = 0): PresenterState {
  return {
    active: false,
    step: clampStep(step, total),
    total: Math.max(0, total),
    scale: DEFAULT_SCALE,
    narrator: false,
    overlays: new Set(),
  };
}

/** Clamp a step index into `[0, total-1]` (or 0 for an empty tour). */
export function clampStep(step: number, total: number): number {
  if (total <= 0) return 0;
  if (step < 0) return 0;
  if (step > total - 1) return total - 1;
  return step;
}

/**
 * A human step counter like `3 / 12` (1-based). An empty tour reads `0 / 0`.
 */
export function stepCounter(state: Pick<PresenterState, 'step' | 'total'>): string {
  if (state.total <= 0) return '0 / 0';
  return `${state.step + 1} / ${state.total}`;
}

/** `true` when the current step is the last one. */
export function isLastStep(state: Pick<PresenterState, 'step' | 'total'>): boolean {
  return state.total > 0 && state.step >= state.total - 1;
}

/** `true` when the current step is the first one. */
export function isFirstStep(state: Pick<PresenterState, 'step'>): boolean {
  return state.step <= 0;
}

/**
 * Cycle to the next font scale preset, wrapping back to the smallest after
 * the largest. Robust to a `scale` that isn't exactly a preset (falls back to
 * the first).
 */
export function nextScale(scale: number): Scale {
  const idx = SCALE_PRESETS.findIndex((s) => s === scale);
  const next = idx === -1 ? 0 : (idx + 1) % SCALE_PRESETS.length;
  return SCALE_PRESETS[next];
}

/**
 * Cycle to the previous font scale preset, wrapping from the smallest to the
 * largest. The `-` key uses this so it actually shrinks the deck (mirror of
 * {@link nextScale}).
 */
export function prevScale(scale: number): Scale {
  const idx = SCALE_PRESETS.findIndex((s) => s === scale);
  const prev = idx === -1 ? 0 : (idx - 1 + SCALE_PRESETS.length) % SCALE_PRESETS.length;
  return SCALE_PRESETS[prev];
}

/**
 * An action the reducer understands. Produced by {@link keyToAction} from a
 * raw key, or dispatched directly by button clicks.
 */
export type PresenterAction =
  | { type: 'enter' }
  | { type: 'exit' }
  | { type: 'next' }
  | { type: 'prev' }
  | { type: 'toggleNarrator' }
  | { type: 'toggleOverlay'; overlay: Overlay }
  | { type: 'cycleScale' }
  | { type: 'cycleScaleDown' }
  | { type: 'setScale'; scale: Scale }
  | { type: 'setStep'; step: number }
  | { type: 'setTotal'; total: number };

/**
 * Map a raw keyboard key to a presenter action, or `null` when the key is not
 * bound. Only consulted while presenter mode is active; the caller is
 * responsible for ignoring keys typed into inputs.
 */
export function keyToAction(key: string): PresenterAction | null {
  switch (key) {
    case 'ArrowRight':
      return { type: 'next' };
    case 'ArrowLeft':
      return { type: 'prev' };
    case 'n':
    case 'N':
      return { type: 'toggleNarrator' };
    case 'r':
    case 'R':
      return { type: 'toggleOverlay', overlay: 'risk' };
    case 'p':
    case 'P':
      return { type: 'toggleOverlay', overlay: 'pattern' };
    case 'Escape':
      return { type: 'exit' };
    case '+':
    case '=':
      return { type: 'cycleScale' };
    case '-':
    case '_':
      return { type: 'cycleScaleDown' };
    case '0':
      return { type: 'setScale', scale: DEFAULT_SCALE };
    default:
      return null;
  }
}

/**
 * Pure reducer. Returns a new {@link PresenterState}; never mutates the input.
 * Navigation clamps to the tour bounds; `next` on the last step (or `prev` on
 * the first) is a no-op rather than an error.
 */
export function reduce(state: PresenterState, action: PresenterAction): PresenterState {
  switch (action.type) {
    case 'enter':
      return { ...state, active: true };
    case 'exit':
      // Leaving the deck also clears transient overlays so re-entering starts
      // clean; the scale/narrator preference persists.
      return { ...state, active: false, overlays: new Set() };
    case 'next':
      return { ...state, step: clampStep(state.step + 1, state.total) };
    case 'prev':
      return { ...state, step: clampStep(state.step - 1, state.total) };
    case 'toggleNarrator':
      return { ...state, narrator: !state.narrator };
    case 'toggleOverlay': {
      const overlays = new Set(state.overlays);
      if (overlays.has(action.overlay)) overlays.delete(action.overlay);
      else overlays.add(action.overlay);
      return { ...state, overlays };
    }
    case 'cycleScale':
      return { ...state, scale: nextScale(state.scale) };
    case 'cycleScaleDown':
      return { ...state, scale: prevScale(state.scale) };
    case 'setScale':
      return { ...state, scale: action.scale };
    case 'setStep':
      return { ...state, step: clampStep(action.step, state.total) };
    case 'setTotal': {
      const total = Math.max(0, action.total);
      return { ...state, total, step: clampStep(state.step, total) };
    }
    default:
      return state;
  }
}
