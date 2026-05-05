/// End-of-tour quiz helpers (#124).
///
/// Pure functions, no DOM. Lifted out of `WalkthroughView.svelte` so vitest
/// can pin the scoring math without booting Svelte.
import type { QuizQuestion } from './api';

/// One answer the user gave during the quiz. `null` for questions the user
/// skipped (the UI doesn't surface "skip" today, but reserving `null` keeps
/// the scoring function future-proof).
export type QuizAnswer = number | null;

/// Quiz outcome rolled up for the result card.
export interface QuizResult {
  /// Number of correct answers.
  correct: number;
  /// Total number of questions in the quiz.
  total: number;
  /// 0-based question indices the user got wrong. The result card uses
  /// this to surface "replay these tour steps" links when each wrong
  /// question carries `step_refs`.
  wrong: number[];
  /// Aggregated step indices to replay — union of `step_refs` across all
  /// wrong questions, deduplicated and sorted ascending.
  replay: number[];
}

/// True when the user's choice matches the question's `answer` index.
/// Returns `false` for `null` (skipped) answers and for malformed quiz
/// entries where `answer` points outside the choices array.
export function isCorrect(question: QuizQuestion, choice: QuizAnswer): boolean {
  if (choice === null) return false;
  if (question.answer < 0 || question.answer >= question.choices.length) return false;
  return choice === question.answer;
}

/// Score a sequence of answers against the quiz. `answers[i]` is the
/// user's choice on `quiz[i]`; arrays must be the same length, but a
/// shorter `answers` array is treated as the user not having answered the
/// trailing questions (each missing slot counts as wrong).
export function scoreQuiz(quiz: QuizQuestion[], answers: QuizAnswer[]): QuizResult {
  const total = quiz.length;
  let correct = 0;
  const wrong: number[] = [];
  const replay = new Set<number>();
  for (let i = 0; i < total; i++) {
    const choice = i < answers.length ? answers[i] : null;
    if (isCorrect(quiz[i], choice)) {
      correct++;
    } else {
      wrong.push(i);
      for (const step of quiz[i].step_refs ?? []) replay.add(step);
    }
  }
  return {
    correct,
    total,
    wrong,
    replay: [...replay].sort((a, b) => a - b),
  };
}
