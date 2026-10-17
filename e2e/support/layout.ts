import { expect, type Locator, type Page } from '@playwright/test';

/** Page scroll height vs. window height: positive when the page scrolls vertically. */
export async function verticalOverflow(page: Page): Promise<number> {
  return page.evaluate(() => document.documentElement.scrollHeight - window.innerHeight);
}

/** Positive when the page scrolls horizontally. */
export async function horizontalOverflow(page: Page): Promise<number> {
  return page.evaluate(() => document.documentElement.scrollWidth - window.innerWidth);
}

/** Asserts that the element's box lies completely inside the window. */
export async function expectInsideViewport(page: Page, el: Locator): Promise<void> {
  const box = await el.boundingBox();
  const size = page.viewportSize()!;
  expect(box, 'element has a box').not.toBeNull();
  expect(box!.x).toBeGreaterThanOrEqual(0);
  expect(box!.y).toBeGreaterThanOrEqual(0);
  expect(box!.x + box!.width).toBeLessThanOrEqual(size.width);
  expect(box!.y + box!.height).toBeLessThanOrEqual(size.height);
}
