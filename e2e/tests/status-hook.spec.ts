import { test, expect } from '../fixtures/weaver';

test.describe('status reflects hook events', () => {
  test('detail view updates the status badge via SSE after a hook', async ({ page, weaver }) => {
    const s = await weaver.seedSession({ goal: 'Watch my status', name: 'hook-detail' });

    await page.goto(`${weaver.baseUrl}/#/s/${s.id}`);
    const badge = page.getByTestId('status-badge').first();
    await expect(badge).toBeVisible();

    // Flip status by shelling out to `weaver hook`; the monitor consumes the
    // resulting event row and the SSE stream pushes the new status through.
    await weaver.hook(s, 'working');
    await expect(badge).toHaveText(/working/i);

    await weaver.hook(s, 'waiting');
    await expect(badge).toHaveText(/waiting/i);
  });

  test('list view picks up a hooked status via polling', async ({ page, weaver }) => {
    const s = await weaver.seedSession({ goal: 'Watch list status', name: 'hook-list' });

    await page.goto(weaver.baseUrl);
    const card = page.locator(`[data-session-id="${s.id}"]`);
    await expect(card.getByTestId('status-badge')).toBeVisible();

    await weaver.hook(s, 'working');

    // The list polls every 3s; allow generous time for it to refresh.
    await expect(card.getByTestId('status-badge')).toHaveText(/working/i, { timeout: 10_000 });
  });
});
