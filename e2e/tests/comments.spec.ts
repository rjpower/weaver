import { test, expect } from '../fixtures/weaver-server';

test('add a comment via UI and see it displayed', async ({ page, weaver }) => {
  // Use a review_request so the issue lands in awaiting_review state,
  // where the comment form shows "Add a comment..." (not revision form)
  const id = await weaver.createIssue({ title: 'Comment Test' });

  weaver.writeProgram(id, [
    { action: 'init' },
    { action: 'review_request', summary: 'Review needed' },
    { action: 'sleep', ms: 500 },
    { action: 'result', result: 'done' },
  ]);

  await weaver.waitForStatus(id, 'awaiting_review', 15_000);

  await page.goto(weaver.baseUrl + `/#/issues/${id}`);

  // Comments section exists
  await expect(page.getByText('Comments')).toBeVisible();

  // Type a comment
  const commentBox = page.locator('textarea[placeholder="Add a comment..."]');
  await commentBox.fill('This is my test comment');
  await page.getByRole('button', { name: 'Post Comment' }).click();

  // Comment should appear
  await expect(page.getByText('This is my test comment')).toBeVisible({ timeout: 5000 });
  await expect(page.getByText('user')).toBeVisible();
});

test('comments added via API appear in the section', async ({ page, weaver }) => {
  weaver.writeDefaultProgram([
    { action: 'init' },
    { action: 'result', result: 'done' },
  ]);

  const id = await weaver.createIssue({ title: 'API Comment Test' });
  await weaver.waitForStatus(id, 'completed');

  // Add comment via API
  await weaver.addComment(id, 'tester', 'Comment from the API');

  await page.goto(weaver.baseUrl + `/#/issues/${id}`);

  await expect(page.getByText('Comment from the API')).toBeVisible();
  await expect(page.getByText('tester')).toBeVisible();
});
