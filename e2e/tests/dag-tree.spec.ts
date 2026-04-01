import { test, expect } from '../fixtures/weaver-server';

test('parent issue creates children that appear in the tree', async ({ page, weaver }) => {
  const parentId = await weaver.createIssue({ title: 'Coordinator Task' });

  weaver.writeProgram(parentId, [
    { action: 'init' },
    { action: 'text', text: 'Creating subtasks...' },
    { action: 'create_issue', title: 'Subtask Alpha', body: 'First subtask' },
    { action: 'create_issue', title: 'Subtask Beta', body: 'Second subtask' },
    { action: 'sleep', ms: 2000 },
    { action: 'result', result: 'Coordinated 2 subtasks' },
  ]);

  weaver.writeDefaultProgram([
    { action: 'init' },
    { action: 'result', result: 'subtask done' },
  ]);

  await weaver.waitForStatus(parentId, 'completed', 20_000);
  // Wait for children to complete
  await page.waitForTimeout(5000);

  await page.goto(weaver.baseUrl + `/#/issues/${parentId}`);

  // Issue title
  await expect(page.locator('h1')).toContainText('Coordinator Task');

  // Children visible in the tree or child list
  await expect(page.getByText('Subtask Alpha')).toBeVisible({ timeout: 10_000 });
  await expect(page.getByText('Subtask Beta')).toBeVisible({ timeout: 10_000 });
});

test('children are visible in the issues list under parent', async ({ page, weaver }) => {
  const parentId = await weaver.createIssue({ title: 'List Parent' });

  weaver.writeProgram(parentId, [
    { action: 'init' },
    { action: 'create_issue', title: 'List Child One', body: 'child' },
    { action: 'result', result: 'done' },
  ]);

  weaver.writeDefaultProgram([
    { action: 'init' },
    { action: 'result', result: 'done' },
  ]);

  await weaver.waitForStatus(parentId, 'completed', 15_000);
  await page.waitForTimeout(5000);

  await page.goto(weaver.baseUrl + '/#/issues');

  await expect(page.getByText('List Parent')).toBeVisible();
  await expect(page.getByText('List Child One')).toBeVisible();
});
