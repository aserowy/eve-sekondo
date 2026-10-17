import { expect, type Page } from '@playwright/test';

// Helpers for reading and hovering the Chart.js canvases the pages draw.

/** Waits until the chart of this kind is drawn: the page may still be navigating. */
export async function chartDrawn(page: Page, kind: string) {
  await page.waitForFunction(
    (kind) => {
      const canvas = document.querySelector(`canvas[data-chart="${kind}"]`);
      return Boolean(canvas && (window as any).Chart?.getChart(canvas));
    },
    kind,
  );
}

/** The page position of the middle of a bar or point of a chart, for hovering it. */
export async function chartPoint(page: Page, kind: string, dataset: number, index: number) {
  await chartDrawn(page, kind);
  return page.evaluate(
    ([kind, dataset, index]) => {
      const canvas = document.querySelector<HTMLCanvasElement>(`canvas[data-chart="${kind}"]`)!;
      const chart = (window as any).Chart.getChart(canvas);
      canvas.scrollIntoView({ block: 'center' });
      const centre = chart.getDatasetMeta(dataset).data[index].getCenterPoint();
      const box = canvas.getBoundingClientRect();
      return { x: box.left + centre.x, y: box.top + centre.y };
    },
    [kind, dataset, index] as const,
  );
}

export async function chartData(page: Page, kind: string): Promise<any[][]> {
  let data: any[][] = [];
  // A live refresh can replace the chart between waiting for it and reading it: read again.
  await expect(async () => {
    await chartDrawn(page, kind);
    data = await page.evaluate((kind) => {
      const canvas = document.querySelector<HTMLCanvasElement>(`canvas[data-chart="${kind}"]`)!;
      return (window as any).Chart.getChart(canvas).data.datasets.map((d: { data: unknown[] }) => d.data);
    }, kind);
  }).toPass();
  return data;
}

export async function hover(page: Page, kind: string, dataset: number, index: number) {
  const { x, y } = await chartPoint(page, kind, dataset, index);
  await page.mouse.move(x, y);
}

/**
 * Hovers a bar or point and expects the chart's tooltip to hold these texts. A live refresh may
 * replace the charts right after the page opens, so hovering and reading are retried together.
 */
export async function expectTooltip(page: Page, kind: string, dataset: number, index: number, texts: (string | RegExp)[]) {
  await expect(async () => {
    await hover(page, kind, dataset, index);
    const tooltip = page.locator(`canvas[data-chart="${kind}"] + .chart-tooltip`);
    for (const text of texts) await expect(tooltip).toContainText(text, { timeout: 1000 });
  }).toPass();
}
