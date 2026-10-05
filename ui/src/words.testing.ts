// For tests: finds the element that reads `text` once its code spans are
// drawn (src/words.tsx) — the words without their backticks, however the
// spans split them into elements — and not an ancestor that only holds it.
export function byWords(text: string) {
  const read = text.replaceAll('`', '');
  return (_: string, element: Element | null) => element !== null && element.textContent === read && ![...element.children].some((child) => child.textContent === read);
}
