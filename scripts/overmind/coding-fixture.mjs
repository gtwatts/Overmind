// A small real coding task, confined to the runner's isolated scratch project.
import { spawn } from 'node:child_process';
import { mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import path from 'node:path';

export const codingTests = `import unittest
from dispatch import format_dispatch

class DispatchTests(unittest.TestCase):
    def test_valid_codes(self):
        for code in ["DISP-0123456789AB", "DISP-ABCDEF123456"]:
            self.assertEqual(format_dispatch(code), "dispatch:" + code)

    def test_reject_invalid_codes(self):
        for code in ["", "DISP-123", "DISP-0123456789AG", "disp-0123456789AB", " DISP-0123456789AB", "DISP-0123456789AB\\n", None, 123]:
            with self.subTest(code=code), self.assertRaises(ValueError):
                format_dispatch(code)
`;

export async function prepareCodingProject(projectDir) {
  await writeFile(path.join(projectDir, 'dispatch.py'), 'def format_dispatch(code):\n    return "dispatch:" + str(code).strip()\n', { mode: 0o600 });
  await writeFile(path.join(projectDir, 'test_dispatch.py'), codingTests, { mode: 0o600 });
}

export function codingTask(ticket) {
  return {
    requiredTools: ['shipping_dispatch'], fields: ['dispatch_code'],
    prompt: `Work only in this isolated scratch Python project. Read dispatch.py and test_dispatch.py. Fix format_dispatch: accept only a string consisting of DISP- followed by exactly 12 uppercase hexadecimal characters, return dispatch: followed by that code, and raise ValueError for every invalid input. Do not modify test_dispatch.py. Run python3 -m unittest -q as a standalone shell command and fix any failures. Use the installed read-only shipping_dispatch fixture tool for ticket ${ticket} to obtain its fresh dispatch_code; do not guess it or use unrelated plugin tools. Also verify format_dispatch against that returned code. Do not install dependencies, access credentials, or browse. After the code and tests succeed, return only the fresh dispatch_code value.`,
  };
}

export async function verifyCodingProject(projectDir, dispatchCode) {
  const intact = await readFile(path.join(projectDir, 'test_dispatch.py'), 'utf8').then((text) => text === codingTests, () => false);
  if (!intact) return { passed: false, test_fixture_unchanged: false, exit_code: null };
  const program = 'import sys,unittest; sys.path.insert(0,"."); import dispatch; assert dispatch.format_dispatch(sys.argv[1]) == "dispatch:" + sys.argv[1]; result=unittest.TextTestRunner(verbosity=0).run(unittest.defaultTestLoader.discover(".", pattern="test_dispatch.py")); sys.exit(not result.wasSuccessful())';
  // -I ignores Python environment flags, and -B alone still reads old .pyc.
  // An empty unique prefix forces both modules to load the current source.
  const cachePrefix = await mkdtemp(path.join(tmpdir(), 'overmind-source-check-'));
  const child = spawn('python3', ['-I', '-B', '-X', `pycache_prefix=${cachePrefix}`, '-c', program, dispatchCode], {
    cwd: projectDir, detached: true, stdio: ['ignore', 'ignore', 'pipe'],
    env: { PATH: process.env.PATH, HOME: projectDir, LANG: 'C.UTF-8' },
  });
  let outputBytes = 0;
  let timedOut = false;
  const kill = () => { try { process.kill(-child.pid, 'SIGKILL'); } catch (error) { if (error.code !== 'ESRCH') throw error; } };
  child.stderr.on('data', (chunk) => { outputBytes += chunk.length; if (outputBytes > 65536) kill(); });
  const timer = setTimeout(() => { timedOut = true; kill(); }, 5000);
  try {
    const exitCode = await new Promise((resolve) => {
      child.once('error', () => resolve(null)); child.once('close', (code) => resolve(code));
    });
    return { passed: exitCode === 0 && !timedOut && outputBytes <= 65536, test_fixture_unchanged: true, exit_code: exitCode, timed_out: timedOut };
  } finally { clearTimeout(timer); await rm(cachePrefix, { recursive: true, force: true }); }
}
