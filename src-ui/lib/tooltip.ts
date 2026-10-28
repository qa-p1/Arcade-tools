// The kit tooltip (STANDARD §5): the action's name in `caption` text after a
// 500 ms hover or on keyboard focus, beside the element. `when` limits it to
// moments the name isn't on screen, such as the collapsed sidebar.
import { motion } from './motion';

type Options = { text: string; when?: () => boolean };

export function tooltip(node: HTMLElement, options: Options) {
  let current = options;
  let timer: ReturnType<typeof setTimeout> | undefined;
  let tip: HTMLDivElement | null = null;

  function show(): void {
    if (tip || (current.when && !current.when())) return;
    tip = document.createElement('div');
    tip.className = 'tooltip';
    tip.setAttribute('role', 'tooltip');
    tip.textContent = current.text;
    document.body.append(tip);
    const box = node.getBoundingClientRect();
    tip.style.left = `${box.right + 8}px`;
    tip.style.top = `${box.top + (box.height - tip.offsetHeight) / 2}px`;
  }
  function hide(): void {
    if (timer) clearTimeout(timer);
    timer = undefined;
    tip?.remove();
    tip = null;
  }
  function later(): void {
    hide();
    timer = setTimeout(show, motion.tooltipDelay);
  }
  node.addEventListener('mouseenter', later);
  node.addEventListener('mouseleave', hide);
  node.addEventListener('focus', show);
  node.addEventListener('blur', hide);
  node.addEventListener('click', hide);
  return {
    update(next: Options) { current = next; if (tip) tip.textContent = next.text; },
    destroy() {
      hide();
      node.removeEventListener('mouseenter', later);
      node.removeEventListener('mouseleave', hide);
      node.removeEventListener('focus', show);
      node.removeEventListener('blur', hide);
      node.removeEventListener('click', hide);
    },
  };
}
