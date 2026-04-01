import { test as base } from '@playwright/test';
import { type ChildProcess, spawn } from 'child_process';
import { chmodSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'fs';
import { tmpdir } from 'os';
import { join } from 'path';

const WEAVER_ROOT = join(__dirname, '..', '..');
const WORKSPACE_ROOT = join(WEAVER_ROOT, '..');
const MOCK_AGENT = join(__dirname, '..', 'mock-agent.py');

function findBinary(): string {
  const fs = require('fs');
  // Cargo workspace puts binaries at the workspace root's target/
  const candidates = [
    join(WORKSPACE_ROOT, 'target', 'release', 'weaver'),
    join(WORKSPACE_ROOT, 'target', 'debug', 'weaver'),
    join(WEAVER_ROOT, 'target', 'release', 'weaver'),
    join(WEAVER_ROOT, 'target', 'debug', 'weaver'),
  ];
  for (const path of candidates) {
    try {
      fs.accessSync(path, fs.constants.X_OK);
      return path;
    } catch {
      // try next
    }
  }
  throw new Error(`weaver binary not found. Run 'cargo build --release' first. Searched: ${candidates.join(', ')}`);
}

export interface WeaverFixture {
  baseUrl: string;
  createIssue(opts: {
    title: string;
    body?: string;
    tags?: string[];
    parent_issue_id?: string;
    priority?: number;
    max_tries?: number;
  }): Promise<string>;
  writeProgram(issueId: string, steps: unknown[]): void;
  writeDefaultProgram(steps: unknown[]): void;
  waitForStatus(issueId: string, status: string, timeoutMs?: number): Promise<void>;
  getIssue(issueId: string): Promise<Record<string, unknown>>;
  addComment(issueId: string, author: string, body: string): Promise<void>;
}

export const test = base.extend<{ weaver: WeaverFixture }>({
  weaver: async ({}, use) => {
    const tmpDir = mkdtempSync(join(tmpdir(), 'weaver-e2e-'));
    const dbPath = join(tmpDir, 'db.sqlite');
    const programsDir = join(tmpDir, 'programs');
    mkdirSync(programsDir);

    chmodSync(MOCK_AGENT, 0o755);

    const binary = findBinary();
    const server = spawn(binary, [
      '--db', dbPath,
      'serve',
      '--addr', '127.0.0.1:0',
    ], {
      env: {
        ...process.env,
        WEAVER_AGENT_BINARY: MOCK_AGENT,
        MOCK_PROGRAMS_DIR: programsDir,
        WEAVER_BINARY_PATH: binary,
        WEAVER_DB_PATH: dbPath,
        RUST_LOG: 'weaver=info',
      },
      stdio: ['ignore', 'pipe', 'pipe'],
    });

    // Parse port from stdout
    let baseUrl = '';
    await new Promise<void>((resolve, reject) => {
      const timeout = setTimeout(() => reject(new Error('Server did not start within 15s')), 15_000);
      let output = '';
      server.stdout!.on('data', (chunk: Buffer) => {
        output += chunk.toString();
        const match = output.match(/listening on (http:\/\/[\d.:]+)/);
        if (match) {
          baseUrl = match[1];
          clearTimeout(timeout);
          resolve();
        }
      });
      server.stderr!.on('data', (chunk: Buffer) => {
        process.stderr.write(`[weaver] ${chunk}`);
      });
      server.on('error', (err) => {
        clearTimeout(timeout);
        reject(err);
      });
      server.on('close', (code) => {
        if (!baseUrl) {
          clearTimeout(timeout);
          reject(new Error(`Server exited with code ${code} before printing address. Output: ${output}`));
        }
      });
    });

    // Wait for API readiness
    for (let i = 0; i < 30; i++) {
      try {
        const resp = await fetch(`${baseUrl}/api/issues`);
        if (resp.ok) break;
      } catch {
        // not ready yet
      }
      await new Promise(r => setTimeout(r, 200));
    }

    const fixture: WeaverFixture = {
      baseUrl,

      async createIssue(opts) {
        const resp = await fetch(`${baseUrl}/api/issues`, {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify(opts),
        });
        if (!resp.ok) {
          throw new Error(`Failed to create issue: ${resp.status} ${await resp.text()}`);
        }
        const data = await resp.json() as { id: string };
        return data.id;
      },

      writeProgram(issueId, steps) {
        writeFileSync(join(programsDir, `${issueId}.json`), JSON.stringify(steps));
      },

      writeDefaultProgram(steps) {
        writeFileSync(join(programsDir, '_default.json'), JSON.stringify(steps));
      },

      async waitForStatus(issueId, status, timeoutMs = 30_000) {
        const start = Date.now();
        while (Date.now() - start < timeoutMs) {
          const resp = await fetch(`${baseUrl}/api/issues/${issueId}`);
          const data = await resp.json() as { status: string };
          if (data.status === status) return;
          await new Promise(r => setTimeout(r, 300));
        }
        // One final check with the actual status for a useful error message
        const resp = await fetch(`${baseUrl}/api/issues/${issueId}`);
        const data = await resp.json() as { status: string };
        throw new Error(`Issue ${issueId} did not reach '${status}' within ${timeoutMs}ms (current: '${data.status}')`);
      },

      async getIssue(issueId) {
        const resp = await fetch(`${baseUrl}/api/issues/${issueId}`);
        return resp.json() as Promise<Record<string, unknown>>;
      },

      async addComment(issueId, author, body) {
        await fetch(`${baseUrl}/api/issues/${issueId}/comments`, {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ author, body }),
        });
      },
    };

    await use(fixture);

    // Cleanup
    server.kill('SIGTERM');
    await new Promise<void>((resolve) => {
      const forceKill = setTimeout(() => {
        server.kill('SIGKILL');
        resolve();
      }, 5000);
      server.on('close', () => {
        clearTimeout(forceKill);
        resolve();
      });
    });
    rmSync(tmpDir, { recursive: true, force: true });
  },
});

export { expect } from '@playwright/test';
