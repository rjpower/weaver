import { test, expect } from '../fixtures/weaver';
import { writeFileSync, mkdirSync } from 'fs';
import { join } from 'path';

// A structured project plan renders on the session Overview: the design + its
// architecture diagram, a task list whose status is projected from the issue
// ledger, a dependency graph, and the Reconcile / Edit actions. This drives a
// real plan file through the running UI.
test.describe('plan on the overview tab', () => {
  const PLAN = [
    '---',
    'plan: feature',
    'status: active',
    '---',
    '',
    '# Feature plan',
    '',
    '## Architecture',
    '```mermaid',
    'flowchart TD',
    '  api --> ui',
    '```',
    '',
    '## Tasks',
    '',
    '### T1 — Build the API  `exec: session`  `value: high`  `deps: —`',
    'The endpoints.',
    '',
    '### T2 — Build the UI  `exec: session`  `value: med`  `deps: T1`',
    'The view.',
    '',
    '### T3 — Tidy up  `exec: inline`  `value: low`  `deps: T2`',
    'Inline only.',
    '',
  ].join('\n');

  test('renders the plan, reconciles it, and opens the editor', async ({ page, weaver }) => {
    const session = await weaver.seedSession({ goal: 'ship the feature', name: 'plan-overview' });

    mkdirSync(join(session.work_dir, 'docs', 'plans'), { recursive: true });
    writeFileSync(join(session.work_dir, 'docs', 'plans', 'feature.md'), PLAN);

    await page.goto(`${weaver.baseUrl}/s/${session.id}`);
    await page.getByRole('button', { name: 'Overview' }).click();

    const plan = page.getByTestId('session-plan');
    await expect(plan.getByText('Feature plan', { exact: true })).toBeVisible();

    // The task list projects status from the ledger; before reconcile the
    // materializing tasks are "planned" (no issue yet).
    await expect(plan.getByText('Build the API')).toBeVisible();
    await expect(plan.getByText('Build the UI')).toBeVisible();
    await expect(plan.getByText('planned').first()).toBeVisible();

    // Both the architecture diagram and the dependency graph render to SVG.
    await expect(plan.locator('.mermaid-diagram svg').first()).toBeVisible();

    // Reconcile previews the delta — create issues for the two session tasks
    // (T3 is inline and never materializes).
    await plan.getByRole('button', { name: 'Reconcile' }).click();
    await expect(plan.getByText(/Proposed changes/)).toBeVisible();
    await expect(plan.getByText(/create issue for T1/)).toBeVisible();
    await expect(plan.getByText(/create issue for T2/)).toBeVisible();
    await expect(plan.getByText(/create issue for T3/)).toHaveCount(0);

    // Apply it; the projected status flips to backlog (open, unclaimed).
    await plan.getByRole('button', { name: /Apply/ }).click();
    await expect(plan.getByText('backlog').first()).toBeVisible();

    // Edit mode flips the rendered plan to an editable Monaco; Cancel returns.
    await plan.getByRole('button', { name: 'Edit' }).click();
    await expect(page.locator('.monaco-editor')).toBeVisible();
    await plan.getByRole('button', { name: 'Cancel' }).click();
    await expect(plan.getByText('Feature plan', { exact: true })).toBeVisible();
  });
});
