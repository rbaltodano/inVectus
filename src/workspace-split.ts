export const PANE_MIN_WIDTH = 400;
export const DIVIDER_WIDTH = 24;

/** Keep both panes usable; stack when the available space cannot fit both minima. */
export function workspaceSplit(width: number, ratio: number) {
  const available = Math.max(0, width - DIVIDER_WIDTH);
  const stacked = available < PANE_MIN_WIDTH * 2;
  const left = stacked ? width : Math.max(PANE_MIN_WIDTH, Math.min(available - PANE_MIN_WIDTH, available * ratio));
  return { stacked, left, right: stacked ? width : available - left, available };
}
