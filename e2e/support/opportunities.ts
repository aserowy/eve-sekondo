import type { Page } from '@playwright/test';
import { expect, feePoll, loginWith, pollCharacter, stationBook, stationSeed, stubControl, stubMarket, stubMarketOrder, stubTradeSkills } from './fixtures';

/** Alice with Accounting 5 and Broker Relations 5 (b 1.41%, t 3.375%) and her orders polled. */
export async function aliceTrained(page: Page) {
  await loginWith(page, 'Alice Alpha');
  await stubControl('skills', { character_id: 90000001, skills: stubTradeSkills(5, 5) });
  expect(await pollCharacter(page, 90000001)).toBe('200 updated');
  expect(await feePoll(page)).toBe('200 ok');
}

/** A buy and a sell order of an item at the station: the buy within region range, so it counts there. */
export function bookOf(type: number, buy: number, sell: number, id: number) {
  return [
    stubMarketOrder(id, { type, price: buy, buy: true, range: 'region' }),
    stubMarketOrder(id + 1, { type, price: sell }),
  ];
}

/** `days` days of history up to yesterday: `average(daysBack)` and a volume of `volume(daysBack)`. */
export function history(type: number, days: number, average: (back: number) => number, volume: (back: number) => number) {
  return Array.from({ length: days }, (_, i) => {
    const back = days - i;
    const avg = average(back);
    return { type_id: type, days_back: back, average: avg, highest: avg * 1.01, lowest: avg * 0.99, volume: volume(back) };
  });
}

/** A calm price with a small wobble: 14 days of about 105 ISK, `volume` traded each day (100 million). */
export const calm = (type: number, volume = 100_000_000) => history(type, 14, (back) => (back % 2 ? 105.5 : 104.5), () => volume);

/** A price that jumped by `jump` three days ago after 27 flat days, with the volume doubling in the last five. */
export const crossed = (type: number, jump: number) =>
  history(type, 30, (back) => (back > 3 ? 100 : 100 * jump), (back) => (back > 5 ? 100_000_000 : 200_000_000));

export async function fill(page: Page, book: ReturnType<typeof stubMarketOrder>[], seed: ReturnType<typeof history>) {
  await stubMarket(book);
  await stubControl('esi-expires', { seconds: 300 });
  expect(await stationBook(page)).toBe('200 updated');
  expect((await stationSeed(page, { history: seed })).startsWith('200 ')).toBe(true);
  await page.reload();
}
