import { test, expect } from '../fixtures/weaver';

test.describe('settings · agent defaults', () => {
  test('agent settings use registry-backed model and effort choices', async ({ page, weaver }) => {
    await page.goto(`${weaver.baseUrl}/settings`);

    const agent = page.getByLabel('Default agent');
    const model = page.getByLabel('Default model');
    const effort = page.getByLabel('Default effort');
    const conciergeAgent = page.getByLabel('Concierge agent');
    const conciergeModel = page.getByLabel('Concierge model');
    const conciergeEffort = page.getByLabel('Concierge effort');

    await expect(agent.locator('option')).toContainText(['Claude', 'Codex', 'Shell']);

    await agent.selectOption('codex');
    await expect(model.locator('option')).toContainText([
      'Default',
      'GPT-5.5',
      'GPT-5.4',
      'GPT-5.4 Mini',
      'GPT-5.3 Codex Spark',
    ]);
    await expect(model.locator('option', { hasText: 'Haiku' })).toHaveCount(0);
    await expect(effort.locator('option')).toContainText([
      'Default',
      'Low',
      'Medium',
      'High',
      'X-High',
    ]);
    await expect(effort.locator('option', { hasText: 'Max' })).toHaveCount(0);

    await agent.selectOption('claude');
    await expect(model.locator('option')).toContainText([
      'Default',
      'Haiku',
      'Sonnet',
      'Opus',
      'Fable',
    ]);
    await expect(effort.locator('option')).toContainText([
      'Default',
      'Low',
      'Medium',
      'High',
      'X-High',
      'Max',
    ]);

    await expect(conciergeAgent.locator('option')).toContainText(['Claude', 'Codex']);
    await expect(conciergeAgent.locator('option', { hasText: 'Shell' })).toHaveCount(0);

    await conciergeAgent.selectOption('codex');
    await expect(conciergeModel.locator('option')).toContainText([
      'Default',
      'GPT-5.5',
      'GPT-5.4',
      'GPT-5.4 Mini',
      'GPT-5.3 Codex Spark',
    ]);
    await expect(conciergeEffort.locator('option')).toContainText([
      'Default',
      'Low',
      'Medium',
      'High',
      'X-High',
    ]);
    await expect(conciergeEffort.locator('option', { hasText: 'Max' })).toHaveCount(0);
  });
});
