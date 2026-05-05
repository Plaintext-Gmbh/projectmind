import { describe, expect, it } from 'vitest';
import type { QuizQuestion } from './api';
import { isCorrect, scoreQuiz } from './quiz';

function q(answer: number, step_refs: number[] = []): QuizQuestion {
  return {
    prompt: 'Which?',
    choices: ['A', 'B', 'C', 'D'],
    answer,
    step_refs,
  };
}

describe('isCorrect', () => {
  it('returns true when the choice matches the answer index', () => {
    expect(isCorrect(q(2), 2)).toBe(true);
  });

  it('returns false for the wrong choice', () => {
    expect(isCorrect(q(2), 0)).toBe(false);
  });

  it('treats null (skipped) as wrong', () => {
    expect(isCorrect(q(2), null)).toBe(false);
  });

  it('treats a malformed quiz answer index as wrong even when the user happens to pick the same number', () => {
    const malformed: QuizQuestion = {
      prompt: 'P',
      choices: ['A'],
      answer: 9,
      step_refs: [],
    };
    expect(isCorrect(malformed, 9)).toBe(false);
  });
});

describe('scoreQuiz', () => {
  it('rolls up correct counts and indices of wrong answers', () => {
    const quiz = [q(0), q(1), q(2)];
    const result = scoreQuiz(quiz, [0, 0, 2]);
    expect(result.correct).toBe(2);
    expect(result.total).toBe(3);
    expect(result.wrong).toEqual([1]);
  });

  it('aggregates and deduplicates replay step refs across wrong questions', () => {
    const quiz = [q(0, [3, 5]), q(1, [5, 7]), q(2, [7])];
    const result = scoreQuiz(quiz, [9, 9, 2]);
    expect(result.correct).toBe(1);
    expect(result.wrong).toEqual([0, 1]);
    expect(result.replay).toEqual([3, 5, 7]);
  });

  it('treats trailing missing answers as wrong', () => {
    const quiz = [q(0), q(1), q(2)];
    const result = scoreQuiz(quiz, [0]);
    expect(result.correct).toBe(1);
    expect(result.wrong).toEqual([1, 2]);
  });

  it('handles an empty quiz', () => {
    expect(scoreQuiz([], [])).toEqual({ correct: 0, total: 0, wrong: [], replay: [] });
  });
});
