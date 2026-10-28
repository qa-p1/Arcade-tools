// Timing from the design kit (STANDARD §4, §5).
export const motion = {
  /** Before a tooltip shows. */
  tooltipDelay: 500,
} as const;

/** Whether the sidebar shows icons only (below 760 px wide). */
export function sidebarCollapsed(): boolean {
  return window.matchMedia('(max-width: 760px)').matches;
}
