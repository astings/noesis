// Home agenda: place a day's meetings as blocks on an hour grid. Pure, local time.

export interface Placed<T> {
  item: T;
  startMin: number; // minutes since midnight
  endMin: number;
  lane: number;
  lanes: number; // lanes used by the overlap cluster this block belongs to
}

// Meetings shorter than this still get a clickable block
const MIN_BLOCK_MIN = 20;

export function layoutDay<T>(items: { item: T; start: Date; seconds?: number }[]): Placed<T>[] {
  const sorted = items
    .map(({ item, start, seconds }) => {
      const startMin = start.getHours() * 60 + start.getMinutes();
      const endMin = Math.min(24 * 60, startMin + Math.max(MIN_BLOCK_MIN, Math.round((seconds ?? 0) / 60)));
      return { item, startMin, endMin, lane: 0, lanes: 1 };
    })
    .sort((a, b) => a.startMin - b.startMin);

  // Greedy lanes within each cluster of transitively overlapping blocks
  let cluster: Placed<T>[] = [];
  let laneEnds: number[] = [];
  let clusterEnd = -1;
  const close = () => cluster.forEach((p) => (p.lanes = laneEnds.length));
  for (const p of sorted) {
    if (p.startMin >= clusterEnd) {
      close();
      cluster = [];
      laneEnds = [];
    }
    let lane = laneEnds.findIndex((end) => end <= p.startMin);
    if (lane === -1) lane = laneEnds.push(0) - 1;
    laneEnds[lane] = p.endMin;
    p.lane = lane;
    cluster.push(p);
    clusterEnd = Math.max(clusterEnd, p.endMin);
  }
  close();
  return sorted;
}
