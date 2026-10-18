import { describe, expect, it } from 'vitest';
import type { ModelRound, TokenUsage } from '../types/flow-chat';
import { getTurnOutputDurationMs, getTurnUsageMetrics } from './turnUsageMetrics';

const usage: TokenUsage = {
  inputTokens: 10_000, cachedTokens: 7_200, outputTokens: 2_800,
  totalTokens: 12_800, timestamp: 1,
};

function round(timing: Pick<ModelRound, 'streamDurationMs' | 'firstChunkMs' | 'firstVisibleOutputMs'>): ModelRound {
  return {
    id: 'round', index: 0, items: [], isStreaming: false, isComplete: true,
    status: 'completed', startTime: 0, ...timing,
  };
}

describe('getTurnUsageMetrics', () => {
  it('uses input tokens as the cache denominator and output tokens as the rate numerator', () => {
    expect(getTurnUsageMetrics(usage, 40_000)).toEqual({
      totalTokens: 12_800, cacheHitRate: 0.72, outputRate: 70, speedLevel: 3,
    });
  });

  it('keeps legacy and unknown values distinct from reported zeros', () => {
    expect(getTurnUsageMetrics(undefined, 1_000)).toEqual({
      totalTokens: null, cacheHitRate: null, outputRate: null, speedLevel: null,
    });
    expect(getTurnUsageMetrics({ ...usage, cachedTokens: undefined, outputTokens: undefined }, 1_000))
      .toMatchObject({ totalTokens: 12_800, cacheHitRate: null, outputRate: null, speedLevel: null });
    expect(getTurnUsageMetrics({ ...usage, cachedTokens: 0, outputTokens: 0 }, 1_000))
      .toMatchObject({ cacheHitRate: 0, outputRate: 0, speedLevel: 1 });
  });

  it.each([undefined, 0, -1, Infinity, NaN])('does not invent throughput for duration %s', duration => {
    expect(getTurnUsageMetrics(usage, duration)).toMatchObject({ outputRate: null, speedLevel: null });
  });

  it.each([[14.9, 1], [15, 2], [39.9, 2], [40, 3], [79.9, 3], [80, 4]])(
    'classifies %s tokens/s without rounding across a tier boundary', (rate, level) => {
      expect(getTurnUsageMetrics({ ...usage, outputTokens: rate * 10 }, 10_000).speedLevel).toBe(level);
    },
  );

  it('rejects inconsistent cache telemetry and non-finite token counts', () => {
    expect(getTurnUsageMetrics({ ...usage, cachedTokens: 10_001 }).cacheHitRate).toBeNull();
    expect(getTurnUsageMetrics({ ...usage, inputTokens: 0 }).cacheHitRate).toBeNull();
    expect(getTurnUsageMetrics({ ...usage, cachedTokens: NaN, totalTokens: Infinity, outputTokens: -1 }, 1_000))
      .toEqual({ totalTokens: null, cacheHitRate: null, outputRate: null, speedLevel: null });
  });

  it('divides by streaming time so tool execution cannot deflate the rate', () => {
    const rounds = [round({ streamDurationMs: 12_000, firstChunkMs: 2_000 })];
    // The turn took 90s wall time; the model only streamed for 10s.
    expect(getTurnUsageMetrics({ ...usage, outputTokens: 2_000 }, getTurnOutputDurationMs(rounds)))
      .toMatchObject({ outputRate: 200, speedLevel: 4 });
  });
});

describe('getTurnOutputDurationMs', () => {
  it('subtracts time to the first upstream item from the stream window', () => {
    expect(getTurnOutputDurationMs([round({ streamDurationMs: 12_000, firstChunkMs: 2_000 })])).toBe(10_000);
  });

  it('falls back to the visible-output mark when the first chunk is unrecorded', () => {
    expect(getTurnOutputDurationMs([round({ streamDurationMs: 9_000, firstVisibleOutputMs: 1_500 })])).toBe(7_500);
  });

  it('uses the whole window when no first-output mark exists', () => {
    expect(getTurnOutputDurationMs([round({ streamDurationMs: 9_000 })])).toBe(9_000);
  });

  it('sums every round of a multi-round turn', () => {
    expect(getTurnOutputDurationMs([
      round({ streamDurationMs: 5_000, firstChunkMs: 1_000 }),
      round({ streamDurationMs: 7_000, firstChunkMs: 2_000 }),
    ])).toBe(9_000);
  });

  it('never reports a negative or invented window', () => {
    expect(getTurnOutputDurationMs([round({ streamDurationMs: 1_000, firstChunkMs: 4_000 })])).toBeUndefined();
    expect(getTurnOutputDurationMs([round({ streamDurationMs: 2_000, firstChunkMs: 2_000 })])).toBeUndefined();
  });

  it('skips rounds without stream timing and reports nothing when none have it', () => {
    expect(getTurnOutputDurationMs([round({}), round({ streamDurationMs: 6_000, firstChunkMs: 1_000 })]))
      .toBe(5_000);
    expect(getTurnOutputDurationMs([round({}), round({})])).toBeUndefined();
    expect(getTurnOutputDurationMs([])).toBeUndefined();
    expect(getTurnOutputDurationMs(undefined)).toBeUndefined();
  });
});
