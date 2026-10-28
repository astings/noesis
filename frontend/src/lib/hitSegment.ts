// same folding as the search index tokenizer (unicode61 remove_diacritics): case and accents ignored
const fold = (s: string) => s.normalize('NFD').replace(/[\u0300-\u036f]/g, '').toLowerCase();

/** Index of the segment to land on for a search hit: in the block, the one holding the most query terms, earliest on ties. */
export function hitSegmentIndex(segments: { timestamp: number; text: string }[], start: number, end: number, query: string): number {
    const terms = fold(query).split(/\s+/).filter(Boolean);
    let first = -1, best = -1, bestScore = 0;
    for (let i = 0; i < segments.length; i++) {
        const ts = segments[i].timestamp;
        if (ts < start - 0.05) continue;
        if (ts > end + 0.05) break;
        if (first < 0) first = i;
        const text = fold(segments[i].text);
        const score = terms.filter((t) => text.includes(t)).length;
        if (score > bestScore) { best = i; bestScore = score; }
    }
    if (best >= 0) return best;
    if (first >= 0) return first;
    // block not found (transcript changed since indexing): last segment starting before the hit
    let i = 0;
    while (i + 1 < segments.length && segments[i + 1].timestamp <= start + 0.05) i++;
    return i;
}
