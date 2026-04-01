import { test, expect } from '../fixtures/weaver-server';

test('shows empty state when no issues exist', async ({ page, weaver }) => {
  await page.goto(weaver.baseUrl + '/#/issues');
  await expect(page.getByText('No issues yet')).toBeVisible();
  await expect(page.getByText('Create an Issue')).toBeVisible();
});

test('displays completed issues in the list', async ({ page, weaver }) => {
  weaver.writeDefaultProgram([
    { action: 'init' },
    { action: 'text', text: 'Working on it...' },
    { action: 'result', result: 'done' },
  ]);

  const id = await weaver.createIssue({ title: 'Test Issue Alpha', body: 'Test body' });
  await weaver.waitForStatus(id, 'completed');

  await page.goto(weaver.baseUrl + '/#/issues');
  await expect(page.getByText('Test Issue Alpha')).toBeVisible();
  // StatusBadge renders status in a span with uppercase styling
  await expect(page.locator('table').getByText('completed')).toBeVisible();
});

test('status filter narrows the list', async ({ page, weaver }) => {
  weaver.writeDefaultProgram([
    { action: 'init' },
    { action: 'result', result: 'done' },
  ]);

  await weaver.createIssue({ title: 'Filter Target' });
  // Wait a moment for the executor to pick it up and complete it
  await page.waitForTimeout(4000);

  await page.goto(weaver.baseUrl + '/#/issues');
  await expect(page.getByText('Filter Target')).toBeVisible();

  // Filter to failed — should hide completed issues
  await page.locator('select').selectOption('failed');
  await page.waitForTimeout(1000);
  await expect(page.getByText('Filter Target')).not.toBeVisible();

  // Filter to completed — should show it again
  await page.locator('select').selectOption('completed');
  await expect(page.getByText('Filter Target')).toBeVisible();
});

test('shows child issues indented with box-drawing character', async ({ page, weaver }) => {
  const parentId = await weaver.createIssue({ title: 'Parent Task' });

  weaver.writeProgram(parentId, [
    { action: 'init' },
    { action: 'create_issue', title: 'Child Task', body: 'I am a child' },
    { action: 'result', result: 'created child' },
  ]);

  weaver.writeDefaultProgram([
    { action: 'init' },
    { action: 'result', result: 'child done' },
  ]);

  await weaver.waitForStatus(parentId, 'completed', 15_000);
  // Wait for child to complete too
  await page.waitForTimeout(5000);

  await page.goto(weaver.baseUrl + '/#/issues');
  await expect(page.getByText('Parent Task')).toBeVisible();
  await expect(page.getByText('Child Task')).toBeVisible();
});

test('clicking an issue navigates to its detail page', async ({ page, weaver }) => {
  weaver.writeDefaultProgram([
    { action: 'init' },
    { action: 'result', result: 'done' },
  ]);

  const id = await weaver.createIssue({ title: 'Clickable Issue' });
  await weaver.waitForStatus(id, 'completed');

  await page.goto(weaver.baseUrl + '/#/issues');
  await page.getByText('Clickable Issue').click();
  await expect(page).toHaveURL(new RegExp(`/issues/${id}`));
  await expect(page.locator('h1')).toContainText('Clickable Issue');
});

test('new issue form creates an issue', async ({ page, weaver }) => {
  weaver.writeDefaultProgram([
    { action: 'init' },
    { action: 'result', result: 'done' },
  ]);

  await page.goto(weaver.baseUrl + '/#/issues');
  await page.getByText('New Issue').click();

  // Fill the form
  await page.locator('input[placeholder*="title" i], input').first().fill('Form Created Issue');
  await page.locator('textarea').first().fill('Created via the dashboard form');
  await page.getByRole('button', { name: 'Create', exact: true }).click();

  // Should navigate to the new issue's detail page
  await expect(page.locator('h1')).toContainText('Form Created Issue');
});
