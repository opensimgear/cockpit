import assert from 'node:assert/strict';
import {execFile} from 'node:child_process';
import {promisify} from 'node:util';
import {resolve} from 'node:path';

const execute = promisify(execFile);

// Read the exact row without acquiring write authority or hashing unrelated rows.
export async function recoveryEvidence(python, directory) {
  const script = [
    'import pathlib,sqlite3,sys',
    'db=sqlite3.connect(pathlib.Path(sys.argv[1]).resolve().as_uri()+"?mode=ro",uri=True)',
    'row=db.execute("SELECT payload FROM records WHERE domain=? AND key=?",("recovery-profile","profile-session-v1.json")).fetchone()',
    'assert row is not None and row[0] is not None, "Missing profile recovery evidence"',
    'print(bytes(row[0]).hex())',
  ].join('\n');
  const {stdout} = await execute(python, ['-c', script, resolve(directory, 'cockpit.sqlite3')], {timeout:5000, maxBuffer:5*1024*1024, windowsHide:true});
  const hex = stdout.trim();
  assert.match(hex, /^[0-9a-f]+$/);
  assert.ok(hex.length <= 4*1024*1024);
  return Buffer.from(hex, 'hex');
}
