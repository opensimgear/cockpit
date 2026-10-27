import test from 'node:test';
import assert from 'node:assert/strict';
import {loadModules} from '../test-module-loader.mjs';
const {ActivityClient,ActivityLogController,activityQuery,emptyActivityFilters,activityFilterCount,activityLabel}= (await loadModules(['./activity/model'])).get('./activity/model');
const plain=value=>JSON.parse(JSON.stringify(value));
const page=(message='found')=>({events:[{id:1,message}],total:1,throughId:1,categories:['profile'],levels:['info'],eventTypes:['profile.updated']});
const deferred=()=>{let resolve,reject;const promise=new Promise((yes,no)=>{resolve=yes;reject=no;});return{promise,resolve,reject};};

test('queries combine full text, multi-select facets, entity and local date ranges without mutating filters',()=>{
  const filters=emptyActivityFilters();filters.categories=['game','profile'];filters.levels=['warning'];filters.eventTypes=['profile.degraded'];filters.entityId=' profile-1 ';filters.time='custom';filters.from='2026-10-26T09:30';filters.until='2026-10-27T10:30';
  const query=activityQuery(filters,'  DCS switch  ',100);
  assert.deepEqual(plain(query),{search:'DCS switch',categories:['game','profile'],levels:['warning'],eventTypes:['profile.degraded'],entityId:'profile-1',since:new Date(filters.from).getTime(),until:new Date(filters.until).getTime(),throughId:null,offset:100,limit:100});
  filters.categories.push('device');assert.equal(query.categories.length,2);assert.equal(activityFilterCount(filters),7);
  assert.equal(activityLabel('profile.recovery-needed'),'Profile Recovery Needed');
});
test('relative time filters and open-ended ranges preserve meaningful boundaries',()=>{
  const now=new Date(2026,9,27,15,30).getTime(),filters=emptyActivityFilters();
  assert.equal(activityFilterCount(filters),0);assert.equal(activityQuery(filters).since,null);assert.equal(activityQuery(filters).entityId,null);
  for(const [time,duration]of [['hour',3600000],['day',86400000],['week',604800000]]){filters.time=time;assert.equal(activityQuery(filters,'',0,now).since,now-duration);}
  filters.time='today';assert.equal(activityQuery(filters,'',0,now).since,new Date(2026,9,27).getTime());
  filters.time='custom';assert.equal(activityQuery(filters).until,null);filters.from='invalid';assert.throws(()=>activityQuery(filters),/valid dates/);
  filters.from='';filters.until='invalid';assert.throws(()=>activityQuery(filters),/valid dates/);
  filters.from='2026-10-27T12:00';filters.until='2026-10-26T12:00';assert.throws(()=>activityQuery(filters),/before/);
  filters.until='';assert.equal(activityQuery(filters).since,new Date(filters.from).getTime());
});
test('client exposes native list, clear and retention with explicit errors and validated settings',async()=>{
  const requests=[];const client=new ActivityClient(async request=>{requests.push(plain(request));return{activity:page(),activityDeleted:42,activitySettings:{retentionDays:request.retentionDays??7}};});
  const query=activityQuery(emptyActivityFilters());assert.equal((await client.list(query)).total,1);assert.equal(await client.clear(),42);assert.equal((await client.settings()).retentionDays,7);assert.equal((await client.saveSettings(30)).retentionDays,30);
  assert.deepEqual(requests.map(item=>item.command),['activity_query','activity_clear','activity_settings_get','activity_settings_save']);
  for(const value of [0,-1,3651,1.5,NaN])await assert.rejects(client.saveSettings(value),/whole number/);
  const missing=new ActivityClient(async()=>({}));await assert.rejects(missing.list(query),/unavailable/);await assert.rejects(missing.settings(),/unavailable/);await assert.rejects(missing.saveSettings(7),/saved/);await assert.rejects(missing.clear(),/cleared/);
});
test('a slower old search cannot replace the current results or report a stale error',async()=>{
  const pending=[];const controller=new ActivityLogController(new ActivityClient(async()=>{const result=deferred();pending.push(result);return result.promise;}));
  let notifications=0;const unsubscribe=controller.subscribe(()=>notifications++);
  const first=controller.load(activityQuery(emptyActivityFilters(),'old')),second=controller.load(activityQuery(emptyActivityFilters(),'new'));
  pending[1].resolve({activity:page('new')});await second;pending[0].reject(Error('old failure'));await first;assert.equal(controller.state.page.events[0].message,'new');assert.equal(controller.state.error,'');assert.equal(controller.state.loading,false);
  const third=controller.load(activityQuery(emptyActivityFilters(),'',100));pending[2].reject(Error('database unavailable'));assert.equal(await third,false);assert.match(controller.state.error,/database unavailable/);assert.equal(controller.state.page.events[0].message,'new');assert.equal(controller.state.query.offset,0,'Failed paging retains the accepted page and position.');
  unsubscribe();const count=notifications;controller.dispose();await controller.load(activityQuery(emptyActivityFilters()));assert.equal(notifications,count);assert.equal(pending.length,3);
});
test('clear invalidates in-flight reads, blocks duplicate deletes, and keeps failure visible',async()=>{
  const pending=[];const client=new ActivityClient(async request=>{const result=deferred();pending.push({command:request.command,...result});return result.promise;});const controller=new ActivityLogController(client);
  const load=controller.load(activityQuery(emptyActivityFilters())),clear=controller.clear();assert.equal(await controller.clear(),false);await controller.load(activityQuery(emptyActivityFilters()));assert.equal(pending.length,2);
  pending[1].resolve({activityDeleted:8});assert.equal(await clear,true);pending[0].resolve({activity:page('deleted')});await load;assert.equal(controller.state.page.total,0);assert.equal(controller.state.page.events.length,0);
  const failed=controller.clear();pending[2].reject(Error('locked'));assert.equal(await failed,false);assert.equal(controller.state.clearing,false);assert.match(controller.state.error,/locked/);
  controller.dispose();assert.equal(await controller.clear(),false);
});
test('disposal cancels late notifications from reads and clear requests',async()=>{
  for(const action of ['load','clear']){const result=deferred(),controller=new ActivityLogController(new ActivityClient(()=>result.promise));let count=0;controller.subscribe(()=>count++);const promise=action==='load'?controller.load(activityQuery(emptyActivityFilters())):controller.clear();controller.dispose();const before=count;result.resolve({activity:page(),activityDeleted:1});await promise;assert.equal(count,before);}
  const controller=new ActivityLogController(new ActivityClient(async()=>{throw 'offline';}));await controller.load(activityQuery(emptyActivityFilters()));assert.equal(controller.state.error,'offline');assert.equal(await controller.clear(),false);assert.equal(controller.state.error,'offline');
});
