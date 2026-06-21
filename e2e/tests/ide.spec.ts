import { test, expect } from '../fixtures/weaver';

// The embedded editor (code-server) lives in a panel pulled in from the right,
// beside the terminal. The proxy/lifecycle is covered by the Rust integration
// test; this drives the UX. Opening the panel must always settle into a valid
// mount — the live editor when code-server is on the host, else a graceful
// "not installed" note — never a broken/blank frame.
test.describe('embedded editor panel', () => {
  test('pulls in from the right, then collapses', async ({ page, weaver }) => {
    const session = await weaver.seedSession({ goal: 'edit some code', name: 'ide-panel' });
    await page.goto(`${weaver.baseUrl}/s/${session.id}`);

    // The collapsed edge handle is the "pull from the right" affordance.
    const handle = page.getByTestId('ide-open');
    await expect(handle).toBeVisible();
    await handle.click();

    // The panel mounts with its header…
    await expect(page.getByText('Editor', { exact: true })).toBeVisible();
    // …and its body settles into a valid state: the live editor iframe when
    // code-server is installed, else the graceful not-installed note (e.g. CI;
    // see docs/embedded-ide.md). Either is correct — neither is a broken frame.
    const liveEditor = page.locator('iframe[title="VS Code"]');
    const notInstalled = page.getByText("code-server isn't installed");
    await expect(liveEditor.or(notInstalled)).toBeVisible();

    // Closing collapses back to the handle.
    await page.getByLabel('Close editor').click();
    await expect(handle).toBeVisible();
  });
});
