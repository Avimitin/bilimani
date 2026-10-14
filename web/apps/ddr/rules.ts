import type { GameRules } from '@bilimani/controller';

/** DDR chart abbreviations: beginner/basic/difficult/expert/challenge, single/double. */
export function ddrRules(): GameRules {
  return {
    parse(text, mode) {
      const match = /(?:^|\s)(bSP|[BDEC][SD]P)$/i.exec(text);
      const token = match?.[1];
      const chart = token === 'bSP' ? 'bSP' : token?.toUpperCase();
      return { query: (match ? text.slice(0, match.index) : text).trim(), mode: chart?.endsWith('DP') ? 'DP' : chart ? 'SP' : mode, chart };
    },
    chartStyle(chart) {
      if (chart === 'bSP') return 'neutral';
      return ({ B: 'amber', D: 'red', E: 'green', C: 'purple' } as Record<string, string>)[chart?.[0] ?? ''] ?? 'neutral';
    },
  };
}
