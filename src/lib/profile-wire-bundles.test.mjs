import test from 'node:test';
import assert from 'node:assert/strict';
import {loadModules} from './test-module-loader.mjs';

const modules=await loadModules(['./profile-wire-bundles']);
const {bundleDiagramRoutes:bundle}=modules.get('./profile-wire-bundles');
const plain=value=>JSON.parse(JSON.stringify(value));
function route(id,{from='source',to='sink',output=id,input=id,start={x:100,y:80},end={x:500,y:80},path=`M100,80 L500,80 ${id}`}={}){
  return {binding:{id,from:{nodeId:from,portId:output},to:{nodeId:to,portId:input}},points:[start,{x:300,y:80},end],path,cornerRadius:24,manual:false,customBlocked:false};
}
const ids=result=>result.map(group=>group.members.map(member=>member.binding.id));

test('different logical signals at the same physical footer anchors use one wire',()=>{
  const first=route('a',{output:'rpm',input:'motor'}),second=route('b',{output:'speed',input:'display'});
  const result=bundle([second,first]);
  assert.equal(result.length,1);
  assert.deepEqual(plain(ids(result)),[['a','b']]);
  assert.equal(result[0].route,first);
  assert.match(result[0].id,/^bundle:/);
  assert.equal(result[0].route.path,first.path,'The existing safe representative geometry is used without rebuilding the route');
});

test('named ports with separate anchor rows remain individually visible',()=>{
  const result=bundle([route('a'),route('b',{end:{x:500,y:108}}),route('c',{start:{x:100,y:108}})]);
  assert.deepEqual(plain(ids(result)),[['a'],['b'],['c']]);
});

test('opposite directions and coincident ports belonging to different nodes stay separate',()=>{
  const result=bundle([route('a'),route('b',{from:'sink',to:'source'}),route('c',{from:'other-source'}),route('d',{to:'other-sink'})]);
  assert.deepEqual(plain(ids(result)),[['a'],['b'],['c'],['d']]);
});

test('selection pulls one signal out with its own path and leaves peers collapsed',()=>{
  const routes=[route('c'),route('b'),route('a')],result=bundle(routes,'b');
  assert.deepEqual(plain(ids(result)),[['a','c'],['b']]);
  assert.equal(result.at(-1).id,'b');
  assert.equal(result.at(-1).route,routes[1]);
  assert.equal(result.at(-1).route.path,routes[1].path);
  assert.equal(result[0].id,bundle(routes)[0].id,'The peer bundle keeps its endpoint identity during selection');
  assert.deepEqual(plain(result.flatMap(group=>group.members).map(member=>member.binding.id).sort()),['a','b','c'],'Every original signal is represented exactly once');
});

test('a two-signal bundle becomes two singleton wires while one signal is selected',()=>{
  const result=bundle([route('b'),route('a')],'a');
  assert.deepEqual(plain(ids(result)),[['b'],['a']]);
  assert.deepEqual(plain(result.map(group=>group.id)),['b','a']);
});

test('representative, member order and bundle identity are independent of input order',()=>{
  const routes=[route('z'),route('a'),route('middle'),route('other',{to:'another-sink'})];
  const first=bundle(routes),reversed=bundle([...routes].reverse());
  assert.deepEqual(plain(first),plain(reversed));
  assert.equal(first[0].route.binding.id,'a');
  assert.equal(first[0].id,bundle([route('new'),...routes])[0].id,'Adding signals does not change a bundle endpoint identity');
});

test('missing or nonfinite endpoints remain standalone without accidental collapse',()=>{
  const routes=[route('a'),route('b')];
  routes[0].points=[];routes[1].points=[{x:NaN,y:80},{x:500,y:80}];
  assert.deepEqual(plain(ids(bundle(routes))),[['a'],['b']]);
});

test('group keys cannot collide when node IDs contain separators or endpoint-like text',()=>{
  const routes=[route('a',{from:'x|y',to:'z'}),route('b',{from:'x',to:'y|z'}),route('c',{from:'x|y',to:'z'})];
  assert.deepEqual(plain(ids(bundle(routes))),[['a','c'],['b']]);
});

test('grouping never mutates input order, route objects, geometry or bindings',()=>{
  const routes=[route('b'),route('a')],before=plain(routes);
  function freeze(value){if(value&&typeof value==='object'){Object.values(value).forEach(freeze);Object.freeze(value);}return value;}
  freeze(routes);
  const result=bundle(routes,'b');
  assert.deepEqual(plain(routes),before);
  assert.equal(result[0].route,routes[1]);
  assert.equal(result[1].route,routes[0]);
});
