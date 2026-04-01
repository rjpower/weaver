import { test, expect } from '../fixtures/weaver-server';

test('shows issue title, status, and metadata', async ({ page, weaver }) => {
  weaver.writeDefaultProgram([
    { action: 'init', model: 'claude-sonnet-4' },
    { action: 'result', result: 'Task completed', input_tokens: 1500, output_tokens: 800, cost_usd: 0.05 },
  ]);

  const id = await weaver.createIssue({
    title: 'Metadata Test Issue',
    body: 'This is the issue body',
    tags: ['impl'],
    priority: 5,
  });
  await weaver.waitForStatus(id, 'completed');

  await page.goto(weaver.baseUrl + `/#/issues/${id}`);

  await expect(page.locator('h1')).toContainText('Metadata Test Issue');
  await expect(page.getByText('completed').first()).toBeVisible();
  // Full ID shown in monospace
  await expect(page.getByText(id)).toBeVisible();
  // MetaGrid cards
  await expect(page.getByText('impl')).toBeVisible();
});

test('shows issue body', async ({ page, weaver }) => {
  weaver.writeDefaultProgram([
    { action: 'init' },
    { action: 'result', result: 'done' },
  ]);

  const id = await weaver.createIssue({
    title: 'Body Test',
    body: 'This body text should be visible in the detail view',
  });
  await weaver.waitForStatus(id, 'completed');

  await page.goto(weaver.baseUrl + `/#/issues/${id}`);
  await expect(page.getByText('This body text should be visible in the detail view')).toBeVisible();
});

test('shows result for completed issues', async ({ page, weaver }) => {
  weaver.writeDefaultProgram([
    { action: 'init' },
    { action: 'result', result: 'The final result output from the agent' },
  ]);

  const id = await weaver.createIssue({ title: 'Result Display Test' });
  await weaver.waitForStatus(id, 'completed');

  await page.goto(weaver.baseUrl + `/#/issues/${id}`);
  // The result section shows the agent's final output (from tagged comment)
  const resultSection = page.locator('pre', { hasText: 'The final result output from the agent' });
  await expect(resultSection).toBeVisible();
  await expect(page.getByText('Result', { exact: true })).toBeVisible();
});

test('shows error for failed issues', async ({ page, weaver }) => {
  const id = await weaver.createIssue({
    title: 'Failing Issue',
    body: 'This will fail',
    max_tries: 1,
  });

  weaver.writeProgram(id, [
    { action: 'init' },
    { action: 'text', text: 'About to fail...' },
    { action: 'fail' },
  ]);

  await weaver.waitForStatus(id, 'failed', 15_000);

  await page.goto(weaver.baseUrl + `/#/issues/${id}`);
  await expect(page.getByText('failed').first()).toBeVisible();
});

test('cancel button stops a running issue', async ({ page, weaver }) => {
  const id = await weaver.createIssue({ title: 'Cancel Me' });

  weaver.writeProgram(id, [
    { action: 'init' },
    { action: 'text', text: 'Starting long work...' },
    { action: 'sleep', ms: 30000 },
    { action: 'result', result: 'done' },
  ]);

  // Navigate while it's running
  await page.goto(weaver.baseUrl + `/#/issues/${id}`);
  await expect(page.getByText('Cancel Issue')).toBeVisible({ timeout: 10_000 });

  // Accept the confirmation dialog
  page.on('dialog', dialog => dialog.accept());
  await page.getByText('Cancel Issue').click();

  await expect(page.getByText('failed').first()).toBeVisible({ timeout: 10_000 });
});
