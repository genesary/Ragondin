// Moving focus as a keyboard user does: Tab (or Shift+Tab) until the target
// holds focus, never a click. A target the keyboard cannot reach within a
// page's worth of stops fails the journey, naming it.
import type { Locator, Page } from '@playwright/test';

const MAX_STOPS = 120;

export async function isFocused(target: Locator): Promise<boolean> {
  return target.evaluate((element) => element === element.ownerDocument.activeElement).catch(() => false);
}

/** Presses `key` until `target` holds focus. */
export async function pressUntilFocused(page: Page, target: Locator, key: string, what: string): Promise<void> {
  for (let stop = 0; stop < MAX_STOPS; stop += 1) {
    if (await isFocused(target)) return;
    await page.keyboard.press(key);
  }
  throw new Error(`${key} never reached ${what}`);
}

/** Tabs forward until `target` holds focus. */
export function tabTo(page: Page, target: Locator, what: string): Promise<void> {
  return pressUntilFocused(page, target, 'Tab', what);
}
