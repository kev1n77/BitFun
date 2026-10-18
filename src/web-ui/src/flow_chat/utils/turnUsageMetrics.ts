import type { ModelRound, TokenUsage } from '../types/flow-chat';

function isCount(value: unknown): value is number {
  return typeof value === 'number' && Number.isFinite(value) && value >= 0;
}

function isPositiveMs(value: unknown): value is number {
  return typeof value === 'number' && Number.isFinite(value) && value > 0;
}

/**
 * Round-level milliseconds spent actually receiving model output.
 *
 * `streamDurationMs` already excludes request handshake and provider queueing,
 * but it still starts before the first token arrives. Subtract the time to the
 * first upstream item so prompt prefill and reasoning warm-up are not charged
 * to generation.
 */
function roundOutputDurationMs(round: ModelRound): number | undefined {
  if (!isPositiveMs(round.streamDurationMs)) return undefined;
  const firstOutputMs = isPositiveMs(round.firstChunkMs) ? round.firstChunkMs
    : isPositiveMs(round.firstVisibleOutputMs) ? round.firstVisibleOutputMs : 0;
  return Math.max(0, round.streamDurationMs - Math.min(firstOutputMs, round.streamDurationMs));
}

/**
 * Milliseconds a turn spent streaming model output, summed over its rounds.
 *
 * Turn wall time also covers tool execution, permission waits and retries, so
 * dividing output tokens by it understates throughput by a wide margin. Returns
 * undefined when no round reports stream timing (older sessions), letting the
 * caller fall back instead of inventing a rate.
 */
export function getTurnOutputDurationMs(rounds?: readonly ModelRound[]): number | undefined {
  if (!rounds?.length) return undefined;
  let totalMs = 0;
  for (const round of rounds) {
    totalMs += roundOutputDurationMs(round) ?? 0;
  }
  return totalMs > 0 ? totalMs : undefined;
}

export function getTurnUsageMetrics(usage?: TokenUsage, outputDurationMs?: number) {
  const totalTokens = isCount(usage?.totalTokens) ? usage.totalTokens : null;
  const cacheHitRate = isCount(usage?.cachedTokens) && isCount(usage?.inputTokens)
    && usage.inputTokens > 0 && usage.cachedTokens <= usage.inputTokens
    ? usage.cachedTokens / usage.inputTokens : null;
  // `outputDurationMs` must be the streaming window from `getTurnOutputDurationMs`,
  // never turn wall time: tool execution and waiting would deflate the rate.
  const rate = isCount(usage?.outputTokens) && isCount(outputDurationMs) && outputDurationMs > 0
    ? usage.outputTokens / (outputDurationMs / 1000) : null;
  const outputRate = rate !== null && Number.isFinite(rate) ? rate : null;
  const speedLevel: 1 | 2 | 3 | 4 | null = outputRate === null ? null
    : outputRate < 15 ? 1 : outputRate < 40 ? 2 : outputRate < 80 ? 3 : 4;
  return { totalTokens, cacheHitRate, outputRate, speedLevel };
}
