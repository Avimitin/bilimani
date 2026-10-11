import type { GameRules } from '@bilimani/controller';
export function iidxRules(): GameRules {
  return {
    parse(text, mode) {
      const match = /(?:^|\s)((?:SP|DP)[BNHAL])$/i.exec(text);
      const chart = match?.[1].toUpperCase();
      const query = (match ? text.slice(0, match.index) : text).trim();
      return { query, mode: chart?.slice(0, 2) ?? mode, chart };
    },
    chartStyle(chart) { return ({ B: 'green', N: 'blue', H: 'amber', A: 'red', L: 'purple' } as Record<string, string>)[chart?.slice(-1) ?? ''] ?? 'neutral'; },
  };
}
