// Read-only acceptance evidence; this never exposes arbitrary SQL to the application.
import {execFile} from 'node:child_process';
import {promisify} from 'node:util';
const execute=promisify(execFile);
export async function recoveryRecord(database,domain,key){
  if(!['recovery-profile','recovery-output','recovery-visibility','recovery-desktop'].includes(domain))throw Error('Choose a recovery domain.');
  const python=process.env.OSG_TEST_PYTHON??'python';
  const script="import sqlite3,json,sys,pathlib; c=sqlite3.connect(pathlib.Path(sys.argv[1]).resolve().as_uri()+'?mode=ro',uri=True); r=c.execute('SELECT revision,payload FROM records WHERE domain=? AND key=?',(sys.argv[2],sys.argv[3])).fetchone(); print(json.dumps(None if r is None or r[1] is None else {'revision':r[0],'payload':json.loads(r[1])}))";
  const {stdout}=await execute(python,['-c',script,database,domain,key],{windowsHide:true,maxBuffer:3*1024*1024});
  return JSON.parse(stdout);
}
