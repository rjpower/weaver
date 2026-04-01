import { test, expect } from '../fixtures/weaver-server';

test('shows live streaming events while issue is running', async ({ page, weaver }) => {
  const id = await weaver.createIssue({ title: 'Streaming Test' });

  weaver.writeProgram(id, [
    { action: 'init', model: 'claude-sonnet-4' },
    { action: 'sleep', ms: 500 },
    { action: 'text', text: 'Analyzing the problem carefully...' },
    { action: 'sleep', ms: 500 },
    { action: 'tool_use', tool: 'Bash', id: 'call_1', input: { command: 'ls -la' } },
    { action: 'sleep', ms: 500 },
    { action: 'tool_result', tool_use_id: 'call_1', output: 'file1.txt\nfile2.txt\nREADME.md' },
    { action: 'sleep', ms: 500 },
    { action: 'text', text: 'Found the files, processing now...' },
    { action: 'sleep', ms: 500 },
    { action: 'result', result: 'All done', input_tokens: 2000, output_tokens: 1000 },
  ]);

  await page.goto(weaver.baseUrl + `/#/issues/${id}`);

  // StreamPanel header
  await expect(page.getByText('Live Output')).toBeVisible({ timeout: 15_000 });

  // Text events appear
  await expect(page.getByText('Analyzing the problem carefully...')).toBeVisible({ timeout: 15_000 });

  // Tool use shows the tool name
  await expect(page.getByText('Bash')).toBeVisible({ timeout: 15_000 });

  // Tool result output
  await expect(page.getByText('file1.txt')).toBeVisible({ timeout: 15_000 });

  // Second text event
  await expect(page.getByText('Found the files, processing now...')).toBeVisible({ timeout: 15_000 });

  // Wait for completion
  await weaver.waitForStatus(id, 'completed', 15_000);

  // After completion, result section appears
  await page.reload();
  // Result text appears in the Result section (from tagged comment)
  await expect(page.locator('pre', { hasText: 'All done' })).toBeVisible({ timeout: 10_000 });
});

test('shows multiple tool calls in sequence', async ({ page, weaver }) => {
  const id = await weaver.createIssue({ title: 'Multi-Tool Test' });

  weaver.writeProgram(id, [
    { action: 'init' },
    { action: 'sleep', ms: 300 },
    { action: 'tool_use', tool: 'Read', id: 'call_1', input: { path: 'src/main.rs' } },
    { action: 'sleep', ms: 300 },
    { action: 'tool_result', tool_use_id: 'call_1', output: 'fn main() { println!("hello"); }' },
    { action: 'sleep', ms: 300 },
    { action: 'tool_use', tool: 'Edit', id: 'call_2', input: { path: 'src/main.rs' } },
    { action: 'sleep', ms: 300 },
    { action: 'tool_result', tool_use_id: 'call_2', output: 'File edited successfully' },
    { action: 'sleep', ms: 300 },
    { action: 'result', result: 'Edited the file' },
  ]);

  await page.goto(weaver.baseUrl + `/#/issues/${id}`);

  await expect(page.getByText('Read', { exact: true })).toBeVisible({ timeout: 15_000 });
  await expect(page.getByText('Edit', { exact: true })).toBeVisible({ timeout: 15_000 });
  await expect(page.getByText('fn main()')).toBeVisible({ timeout: 15_000 });
  await expect(page.getByText('File edited successfully')).toBeVisible({ timeout: 15_000 });
});
