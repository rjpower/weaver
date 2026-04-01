import { test, expect } from '../fixtures/weaver-server';

test('review banner appears for awaiting_review issues', async ({ page, weaver }) => {
  const id = await weaver.createIssue({ title: 'Review Me' });

  weaver.writeProgram(id, [
    { action: 'init' },
    { action: 'text', text: 'Work complete, requesting review.' },
    { action: 'review_request', summary: 'Please review my changes' },
    { action: 'sleep', ms: 500 },
    { action: 'result', result: 'Submitted for review' },
  ]);

  await weaver.waitForStatus(id, 'awaiting_review', 15_000);

  await page.goto(weaver.baseUrl + `/#/issues/${id}`);

  await expect(page.getByText('This issue is awaiting your review')).toBeVisible({ timeout: 10_000 });
  await expect(page.getByRole('button', { name: 'Approve' })).toBeVisible();
  await expect(page.getByRole('button', { name: 'Request Changes' })).toBeVisible();
});

test('approving transitions issue to completed', async ({ page, weaver }) => {
  const id = await weaver.createIssue({ title: 'Approve Me' });

  weaver.writeProgram(id, [
    { action: 'init' },
    { action: 'review_request', summary: 'Ready for approval' },
    { action: 'sleep', ms: 500 },
    { action: 'result', result: 'Done pending approval' },
  ]);

  await weaver.waitForStatus(id, 'awaiting_review', 15_000);

  await page.goto(weaver.baseUrl + `/#/issues/${id}`);
  await expect(page.getByText('This issue is awaiting your review')).toBeVisible({ timeout: 10_000 });

  await page.getByRole('button', { name: 'Approve' }).click();

  // Status should transition to completed
  await expect(page.getByText('completed').first()).toBeVisible({ timeout: 10_000 });
});
