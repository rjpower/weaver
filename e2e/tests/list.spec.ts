import { test, expect } from '../fixtures/weaver';

test.describe('session list view', () => {
  test('shows an empty state when there are no sessions', async ({ page, weaver }) => {
    await page.goto(weaver.baseUrl);
    await expect(page.getByRole('heading', { name: 'Sessions' })).toBeVisible();
    await expect(page.getByText('No sessions yet.')).toBeVisible();
    await expect(page.getByTestId('session-card')).toHaveCount(0);
  });

  test('renders seeded sessions with name, status and goal', async ({ page, weaver }) => {
    const a = await weaver.seedSession({ goal: 'Add a health endpoint', name: 'alpha-task' });
    const b = await weaver.seedSession({ goal: 'Fix the login bug', name: 'beta-task' });

    await page.goto(weaver.baseUrl);

    const cards = page.getByTestId('session-card');
    await expect(cards).toHaveCount(2);

    const cardA = page.locator(`[data-session-id="${a.id}"]`);
    await expect(cardA).toContainText('alpha-task');
    await expect(cardA).toContainText('Add a health endpoint');
    await expect(cardA.getByTestId('status-badge')).toBeVisible();

    const cardB = page.locator(`[data-session-id="${b.id}"]`);
    await expect(cardB).toContainText('beta-task');
    await expect(cardB).toContainText('Fix the login bug');
  });

  test('clicking a card navigates to the detail view', async ({ page, weaver }) => {
    const s = await weaver.seedSession({ goal: 'Navigate to me', name: 'nav-task' });

    await page.goto(weaver.baseUrl);
    await page.locator(`[data-session-id="${s.id}"]`).click();

    await expect(page).toHaveURL(new RegExp(`#/s/${s.id}$`));
    await expect(page.getByRole('heading', { name: 'nav-task' })).toBeVisible();
    await expect(page.locator('textarea').first()).toHaveValue('Navigate to me');
  });
});
